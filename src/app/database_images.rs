use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{Cassie, CassieError};
use crate::midge::adapter::StagedDatabaseFamily;

pub const DATABASE_IMAGE_VERSION: u16 = 1;
const DATABASE_IMAGE_LAYOUT_VERSION: &str = "cassie-midge-layout-v1";
const DATABASE_IMAGE_MAGIC: &[u8] = b"CASSIEDB";
const FRAME_HEADER: u8 = b'H';
const FRAME_CATALOG: u8 = b'C';
const FRAME_DATA: u8 = b'D';
const FRAME_FOOTER: u8 = b'F';
const IMAGE_CHUNK_BYTES: usize = 64 * 1024;
const MAX_IMAGE_FRAME_BYTES: usize = 64 * 1024 * 1024;
const IMAGE_SCAN_PAGE_ENTRIES: usize = 256;
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DatabaseImageHeader {
    image_version: u16,
    source_database: String,
    source_physical_family: String,
    schema_epoch: u64,
    data_epoch: u64,
    storage_layout_version: String,
    integrity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DatabaseImageFooter {
    catalog_entries: u64,
    data_entries: u64,
    checksum_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackupPhase {
    Header,
    Catalog,
    Data,
    Footer,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestorePhase {
    Magic,
    Header,
    Catalog,
    Data,
    Footer,
    Closed,
}

/// A bounded-chunk logical backup stream. Catalog records are rewritten by
/// restore; data-family keys are already database-local and are copied raw.
pub struct DatabaseBackupStream {
    cassie: Cassie,
    source_database: String,
    source_physical_family: String,
    schema_epoch: u64,
    data_epoch: u64,
    catalog_entries: Vec<(Vec<u8>, Vec<u8>)>,
    catalog_index: usize,
    data_cursor: Option<Vec<u8>>,
    data_page: VecDeque<(Vec<u8>, Vec<u8>)>,
    data_exhausted: bool,
    pending: VecDeque<u8>,
    phase: BackupPhase,
    hasher: Sha256,
    catalog_count: u64,
    data_count: u64,
}

impl DatabaseBackupStream {
    /// Return the next length-delimited image chunk, or `None` at EOF.
    ///
    /// # Errors
    ///
    /// Returns an error if a frame cannot be encoded.
    pub fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, CassieError> {
        if matches!(self.phase, BackupPhase::Finished) && self.pending.is_empty() {
            return Ok(None);
        }

        while self.pending.len() < IMAGE_CHUNK_BYTES {
            match self.phase {
                BackupPhase::Header => {
                    self.pending.extend(DATABASE_IMAGE_MAGIC.iter().copied());
                    let header = DatabaseImageHeader {
                        image_version: DATABASE_IMAGE_VERSION,
                        source_database: self.source_database.clone(),
                        source_physical_family: self.source_physical_family.clone(),
                        storage_layout_version: DATABASE_IMAGE_LAYOUT_VERSION.to_string(),
                        schema_epoch: self.schema_epoch,
                        data_epoch: self.data_epoch,
                        integrity: "sha256(catalog-and-data-frame-payloads)".to_string(),
                    };
                    self.enqueue_frame(
                        FRAME_HEADER,
                        &serde_json::to_vec(&header).map_err(|error| {
                            CassieError::Parse(format!("encode database image header: {error}"))
                        })?,
                    );
                    self.phase = BackupPhase::Catalog;
                }
                BackupPhase::Catalog => {
                    if let Some((key, value)) = self.catalog_entries.get(self.catalog_index) {
                        let frame = encode_entry_frame(key, value)?;
                        self.hasher
                            .update(frame_integrity_bytes(FRAME_CATALOG, key, value));
                        self.catalog_count = self.catalog_count.saturating_add(1);
                        self.catalog_index += 1;
                        self.enqueue_frame(FRAME_CATALOG, &frame);
                    } else {
                        self.phase = BackupPhase::Data;
                    }
                }
                BackupPhase::Data => {
                    if self.data_page.is_empty() && !self.data_exhausted {
                        let page = self.cassie.midge.raw_scan_database_page(
                            &self.source_database,
                            b"",
                            self.data_cursor.as_deref(),
                            IMAGE_SCAN_PAGE_ENTRIES,
                        )?;
                        if page.is_empty() {
                            self.data_exhausted = true;
                        } else {
                            self.data_cursor = page.last().map(|(key, _)| key.clone());
                            self.data_page.extend(page);
                        }
                    }

                    if let Some((key, value)) = self.data_page.pop_front() {
                        let frame = encode_entry_frame(&key, &value)?;
                        self.hasher.update(frame_integrity_bytes(
                            FRAME_DATA,
                            key.as_slice(),
                            value.as_slice(),
                        ));
                        self.data_count = self.data_count.saturating_add(1);
                        self.enqueue_frame(FRAME_DATA, &frame);
                    } else {
                        self.phase = BackupPhase::Footer;
                    }
                }
                BackupPhase::Footer => {
                    let footer = DatabaseImageFooter {
                        catalog_entries: self.catalog_count,
                        data_entries: self.data_count,
                        checksum_sha256: hex_digest(&self.hasher),
                    };
                    self.enqueue_frame(
                        FRAME_FOOTER,
                        &serde_json::to_vec(&footer).map_err(|error| {
                            CassieError::Parse(format!("encode database image footer: {error}"))
                        })?,
                    );
                    self.phase = BackupPhase::Finished;
                }
                BackupPhase::Finished => break,
            }
        }

        if self.pending.is_empty() {
            return Ok(None);
        }
        let length = self.pending.len().min(IMAGE_CHUNK_BYTES);
        Ok(Some(self.pending.drain(..length).collect()))
    }

    fn enqueue_frame(&mut self, tag: u8, payload: &[u8]) {
        let body_len = payload.len().saturating_add(1);
        self.pending
            .extend((u32::try_from(body_len).unwrap_or(u32::MAX)).to_be_bytes());
        self.pending.push_back(tag);
        self.pending.extend(payload.iter().copied());
    }
}

pub struct DatabaseRestoreSession {
    cassie: Cassie,
    staged: Option<StagedDatabaseFamily>,
    input: Vec<u8>,
    phase: RestorePhase,
    header: Option<DatabaseImageHeader>,
    catalog_entries: Vec<(Vec<u8>, Vec<u8>)>,
    hasher: Sha256,
    catalog_count: u64,
    data_count: u64,
}

impl DatabaseRestoreSession {
    /// Push one arbitrary fragment of the image stream.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed, truncated, incompatible, or corrupt
    /// frames. An error leaves the staged target invisible and abortable.
    pub fn push_chunk(&mut self, chunk: &[u8]) -> Result<(), CassieError> {
        if matches!(self.phase, RestorePhase::Closed) {
            return Err(CassieError::Execution(
                "database restore session is already closed".to_string(),
            ));
        }
        self.input.extend_from_slice(chunk);
        loop {
            if matches!(self.phase, RestorePhase::Magic) {
                if self.input.len() < DATABASE_IMAGE_MAGIC.len() {
                    return Ok(());
                }
                if self.input[..DATABASE_IMAGE_MAGIC.len()] != *DATABASE_IMAGE_MAGIC {
                    return Err(CassieError::Parse(
                        "invalid database image magic".to_string(),
                    ));
                }
                self.input.drain(..DATABASE_IMAGE_MAGIC.len());
                self.phase = RestorePhase::Header;
            }
            if matches!(self.phase, RestorePhase::Footer) {
                return if self.input.is_empty() {
                    Ok(())
                } else {
                    Err(CassieError::Parse(
                        "database image contains data after footer".to_string(),
                    ))
                };
            }
            if self.input.len() < 4 {
                return Ok(());
            }
            let body_len =
                u32::from_be_bytes(self.input[..4].try_into().map_err(|_| {
                    CassieError::Parse("invalid database image frame length".into())
                })?) as usize;
            if body_len == 0 || body_len > MAX_IMAGE_FRAME_BYTES {
                return Err(CassieError::Parse(
                    "database image frame length is invalid".to_string(),
                ));
            }
            if self.input.len() < body_len.saturating_add(4) {
                return Ok(());
            }
            let body = self.input[4..4 + body_len].to_vec();
            self.input.drain(..4 + body_len);
            let tag = body[0];
            let payload = &body[1..];
            match tag {
                FRAME_HEADER => self.accept_header(payload)?,
                FRAME_CATALOG => self.accept_catalog(payload)?,
                FRAME_DATA => self.accept_data(payload)?,
                FRAME_FOOTER => self.accept_footer(payload)?,
                _ => {
                    return Err(CassieError::Parse(
                        "unknown database image frame".to_string(),
                    ))
                }
            }
        }
    }

    /// Commit the staged family and catalog records, making the target visible.
    ///
    /// # Errors
    ///
    /// Returns an error if the image is incomplete or the final catalog commit
    /// fails. The staged family remains abortable on error.
    pub fn finish(&mut self) -> Result<(), CassieError> {
        if matches!(self.phase, RestorePhase::Closed) {
            return Err(CassieError::Execution(
                "database restore session is already closed".to_string(),
            ));
        }
        if !self.input.is_empty()
            || self.header.is_none()
            || !matches!(self.phase, RestorePhase::Footer)
        {
            return Err(CassieError::Parse(
                "database image ended before a complete footer".to_string(),
            ));
        }
        let staged = self.staged.take().ok_or_else(|| {
            CassieError::Execution("database restore staging is unavailable".to_string())
        })?;
        let header = self
            .header
            .as_ref()
            .ok_or_else(|| CassieError::Parse("database image header is missing".to_string()))?;
        let result = self.cassie.midge.commit_staged_database_family(
            staged,
            &header.source_database,
            &header.source_physical_family,
            std::mem::take(&mut self.catalog_entries),
        );
        self.phase = RestorePhase::Closed;
        result
    }

    /// Remove staged data and journal state without exposing the target.
    ///
    /// # Errors
    ///
    /// Returns an error if staged cleanup cannot be persisted.
    pub fn abort(&mut self) -> Result<(), CassieError> {
        if matches!(self.phase, RestorePhase::Closed) {
            return Ok(());
        }
        self.phase = RestorePhase::Closed;
        if let Some(staged) = self.staged.take() {
            self.cassie.midge.abort_staged_database_family(&staged)?;
        }
        Ok(())
    }

    fn accept_header(&mut self, payload: &[u8]) -> Result<(), CassieError> {
        if !matches!(self.phase, RestorePhase::Header) {
            return Err(CassieError::Parse(
                "database image contains multiple headers".to_string(),
            ));
        }
        let header: DatabaseImageHeader = serde_json::from_slice(payload).map_err(|error| {
            CassieError::Parse(format!("invalid database image header: {error}"))
        })?;
        if header.image_version != DATABASE_IMAGE_VERSION
            || header.storage_layout_version != DATABASE_IMAGE_LAYOUT_VERSION
        {
            return Err(CassieError::Unsupported(
                "database image version or storage layout is incompatible".to_string(),
            ));
        }
        self.header = Some(header);
        self.phase = RestorePhase::Catalog;
        Ok(())
    }

    fn accept_catalog(&mut self, payload: &[u8]) -> Result<(), CassieError> {
        if !matches!(self.phase, RestorePhase::Catalog) {
            return Err(CassieError::Parse(
                "database image catalog appeared after data".to_string(),
            ));
        }
        let (key, value) = decode_entry_frame(payload)?;
        self.hasher
            .update(frame_integrity_bytes(FRAME_CATALOG, &key, &value));
        self.catalog_count = self.catalog_count.saturating_add(1);
        self.catalog_entries.push((key, value));
        Ok(())
    }

    fn accept_data(&mut self, payload: &[u8]) -> Result<(), CassieError> {
        if !matches!(self.phase, RestorePhase::Catalog | RestorePhase::Data) {
            return Err(CassieError::Parse(
                "database image data appeared before a valid header".to_string(),
            ));
        }
        let (key, value) = decode_entry_frame(payload)?;
        let staged = self.staged.as_ref().ok_or_else(|| {
            CassieError::Parse("database image data appeared before a valid header".to_string())
        })?;
        self.cassie
            .midge
            .write_staged_database_entries(staged, &[(key.clone(), value.clone())])?;
        self.hasher
            .update(frame_integrity_bytes(FRAME_DATA, &key, &value));
        self.data_count = self.data_count.saturating_add(1);
        self.phase = RestorePhase::Data;
        Ok(())
    }

    fn accept_footer(&mut self, payload: &[u8]) -> Result<(), CassieError> {
        if !matches!(self.phase, RestorePhase::Catalog | RestorePhase::Data) {
            return Err(CassieError::Parse(
                "database image contains multiple footers".to_string(),
            ));
        }
        let footer: DatabaseImageFooter = serde_json::from_slice(payload).map_err(|error| {
            CassieError::Parse(format!("invalid database image footer: {error}"))
        })?;
        if footer.catalog_entries != self.catalog_count
            || footer.data_entries != self.data_count
            || footer.checksum_sha256 != hex_digest(&self.hasher)
        {
            return Err(CassieError::Storage(
                "database image checksum or entry count mismatch".to_string(),
            ));
        }
        self.phase = RestorePhase::Footer;
        Ok(())
    }
}

impl Drop for DatabaseRestoreSession {
    fn drop(&mut self) {
        let _ = self.abort();
    }
}

impl Cassie {
    /// Begin a logical, database-scoped streaming backup.
    ///
    /// # Errors
    ///
    /// Returns an error if the source database or its family is unavailable.
    pub fn begin_database_backup(
        &self,
        database: &str,
    ) -> Result<DatabaseBackupStream, CassieError> {
        let metadata = self.midge.get_database(database)?.ok_or_else(|| {
            CassieError::NotFound(format!("database '{database}' does not exist"))
        })?;
        Ok(DatabaseBackupStream {
            cassie: self.clone(),
            source_database: metadata.name.clone(),
            source_physical_family: metadata.physical_family,
            schema_epoch: self.midge.schema_epoch()?,
            data_epoch: self.midge.data_epoch_for_database(database)?,
            catalog_entries: self.midge.database_catalog_entries(database)?,
            catalog_index: 0,
            data_cursor: None,
            data_page: VecDeque::new(),
            data_exhausted: false,
            pending: VecDeque::new(),
            phase: BackupPhase::Header,
            hasher: Sha256::new(),
            catalog_count: 0,
            data_count: 0,
        })
    }

    /// Begin a restore into a new logical database. The staged family is
    /// journaled and invisible until `finish` commits the catalog.
    ///
    /// # Errors
    ///
    /// Returns an error if the target already exists or staging cannot start.
    pub fn begin_database_restore(
        &self,
        target_database: &str,
    ) -> Result<DatabaseRestoreSession, CassieError> {
        let staged = self.midge.stage_database_family(target_database)?;
        Ok(DatabaseRestoreSession {
            cassie: self.clone(),
            staged: Some(staged),
            input: Vec::new(),
            phase: RestorePhase::Magic,
            header: None,
            catalog_entries: Vec::new(),
            hasher: Sha256::new(),
            catalog_count: 0,
            data_count: 0,
        })
    }
}

fn encode_entry_frame(key: &[u8], value: &[u8]) -> Result<Vec<u8>, CassieError> {
    let key_len = u32::try_from(key.len())
        .map_err(|_| CassieError::Unsupported("database image key is too large".to_string()))?;
    let value_len = u32::try_from(value.len())
        .map_err(|_| CassieError::Unsupported("database image value is too large".to_string()))?;
    let mut payload = Vec::with_capacity(8 + key.len() + value.len());
    payload.extend_from_slice(&key_len.to_be_bytes());
    payload.extend_from_slice(&value_len.to_be_bytes());
    payload.extend_from_slice(key);
    payload.extend_from_slice(value);
    Ok(payload)
}

fn decode_entry_frame(payload: &[u8]) -> Result<(Vec<u8>, Vec<u8>), CassieError> {
    if payload.len() < 8 {
        return Err(CassieError::Parse(
            "database image entry frame is truncated".to_string(),
        ));
    }
    let key_len = u32::from_be_bytes(
        payload[..4]
            .try_into()
            .map_err(|_| CassieError::Parse("invalid database image key length".to_string()))?,
    ) as usize;
    let value_len = u32::from_be_bytes(
        payload[4..8]
            .try_into()
            .map_err(|_| CassieError::Parse("invalid database image value length".to_string()))?,
    ) as usize;
    let expected = 8usize
        .checked_add(key_len)
        .and_then(|length| length.checked_add(value_len))
        .ok_or_else(|| CassieError::Parse("database image entry length overflow".to_string()))?;
    if expected != payload.len() {
        return Err(CassieError::Parse(
            "database image entry frame has invalid lengths".to_string(),
        ));
    }
    Ok((
        payload[8..8 + key_len].to_vec(),
        payload[8 + key_len..].to_vec(),
    ))
}

fn frame_integrity_bytes(tag: u8, key: &[u8], value: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(1 + 8 + key.len() + value.len());
    bytes.push(tag);
    bytes.extend_from_slice(&(u64::try_from(key.len()).unwrap_or(u64::MAX)).to_be_bytes());
    bytes.extend_from_slice(&(u64::try_from(value.len()).unwrap_or(u64::MAX)).to_be_bytes());
    bytes.extend_from_slice(key);
    bytes.extend_from_slice(value);
    bytes
}

fn hex_digest(hasher: &Sha256) -> String {
    let digest = hasher.clone().finalize();
    let mut output = String::with_capacity(digest.len().saturating_mul(2));
    for byte in digest {
        output.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}
