//! COPY physical identity from its staged overlay or the current family.

use std::io::{self, Read};

use super::{CassieError, Midge};
use crate::adapter::StorageRequestContext;
use crate::adapter::{StorageOverlay, TransactionMode};

#[cfg(test)]
#[path = "copy_identity_tests.rs"]
mod tests;

struct CheckingReader<'a> {
    input: &'a [u8],
    position: usize,
    controls: &'a StorageRequestContext<'a>,
}

impl Read for CheckingReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.position.is_multiple_of(4096) {
            self.controls
                .check_interruption()
                .map_err(|_| io::Error::other("query interrupted"))?;
        }
        let count = output
            .len()
            .min(self.input.len() - self.position)
            .min(4096 - self.position % 4096);
        output[..count].copy_from_slice(&self.input[self.position..self.position + count]);
        self.position += count;
        Ok(count)
    }
}

#[derive(serde::Deserialize)]
struct Identity {
    #[serde(default)]
    relation_id: u64,
}

pub(super) fn decode_identity(
    raw: &[u8],
    controls: &StorageRequestContext<'_>,
) -> Result<Option<u64>, CassieError> {
    controls.check_interruption()?;
    let _memory = controls.reserve_query_memory(
        cassie_core::request::accounted::json::serialized_decode_bytes(raw.len())?,
    )?;
    let reader = CheckingReader {
        input: raw,
        position: 0,
        controls,
    };
    let identity: Identity = serde_json::from_reader(reader).map_err(|error| {
        controls
            .check_interruption()
            .err()
            .unwrap_or_else(|| CassieError::Parse(format!("invalid COPY row descriptor: {error}")))
    })?;
    Ok((identity.relation_id != 0).then_some(identity.relation_id))
}

impl Midge {
    pub fn copy_physical_identity(
        &self,
        name: &str,
        overlay: &StorageOverlay,
        controls: &StorageRequestContext<'_>,
    ) -> Result<Option<u64>, CassieError> {
        controls.check_interruption()?;
        let canonical = self.canonical_collection_name(name);
        let key = Self::row_schema_key(&canonical);
        if let Some(change) = overlay.changes.get(&key) {
            return change
                .value
                .as_ref()
                .map(|raw| decode_identity(raw, controls))
                .transpose()
                .map(Option::flatten);
        }
        let database = crate::catalog::relation_database_name(&canonical)
            .unwrap_or_else(|| self.default_database.clone());
        let tx = self.begin_raw_database_adapter_tx(&database, TransactionMode::ReadOnly)?;
        let Some(raw) = tx.get(&key).map_err(CassieError::from)? else {
            return Ok(None);
        };
        let _raw_memory = controls.reserve_query_memory(raw.len())?;
        decode_identity(&raw, controls)
    }
}

#[cfg(feature = "storage-test-hooks")]
impl Midge {
    /// Enters the native transaction boundary used to seed a legacy format.
    ///
    /// # Errors
    /// Returns an error if storage cannot commit the fixture.
    #[doc(hidden)]
    pub fn plant_legacy_copy_identity_row(
        &self,
        collection: &str,
        id: &str,
        bytes: Vec<u8>,
    ) -> Result<(), CassieError> {
        let mut tx = self.transaction(
            crate::adapter::StorageFamily::Data,
            TransactionMode::ReadWrite,
        )?;
        tx.put(
            crate::adapter::key_encoding::doc_key(collection, id),
            bytes,
            None,
        )?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)
    }

    /// Returns the exact persisted row key prefix for a retained-read fixture.
    #[must_use]
    #[doc(hidden)]
    pub fn row_prefix_for_copy_identity_test(relation_id: u64) -> Vec<u8> {
        Self::row_prefix(relation_id)
    }

    /// Returns the persisted row descriptor for a retained-read fixture.
    ///
    /// # Errors
    /// Returns an error if storage cannot read the descriptor.
    #[doc(hidden)]
    pub fn row_schema_for_copy_identity_test(
        &self,
        collection: &str,
    ) -> Result<crate::row_blob::RowSchema, CassieError> {
        self.row_schema(collection)
    }
}
