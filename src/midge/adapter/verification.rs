use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;

const ROW_HASH_ALGORITHM: &str = "cassie-fnv128";
const ROW_HASH_DIGEST_LENGTH: u16 = 16;
const CANONICAL_ENCODER_VERSION: u16 = 1;
const ROW_HASH_VERSION: u16 = 1;
const RANGE_HASH_VERSION: u16 = 1;
const ROOT_HASH_VERSION: u16 = 1;
const RANGE_SEGMENT_SIZE: usize = 256;
const EAGER_HASH_REBUILD_ROW_LIMIT: u64 = 512;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StoredHashState {
    Current,
    Stale,
    Incomplete,
    Incompatible,
    Empty,
    Tombstone,
}

impl StoredHashState {
    fn as_projection_state(&self) -> crate::catalog::ProjectionVerificationState {
        match self {
            Self::Current => crate::catalog::ProjectionVerificationState::Current,
            Self::Stale => crate::catalog::ProjectionVerificationState::Stale,
            Self::Incomplete => crate::catalog::ProjectionVerificationState::Incomplete,
            Self::Incompatible => crate::catalog::ProjectionVerificationState::Incompatible,
            Self::Empty => crate::catalog::ProjectionVerificationState::Empty,
            Self::Tombstone => crate::catalog::ProjectionVerificationState::Missing,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RowHashRecord {
    pub projection_id: String,
    pub version_id: Option<String>,
    pub collection: String,
    pub schema_epoch: u64,
    pub row_id: String,
    pub algorithm: String,
    pub digest_length: u16,
    pub canonical_encoder_version: u16,
    pub row_hash_version: u16,
    pub digest: String,
    pub state: StoredHashState,
    pub computed_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RangeHashRecord {
    pub projection_id: String,
    pub version_id: Option<String>,
    pub collection: String,
    pub schema_epoch: u64,
    pub range_id: u64,
    pub first_row_id: Option<String>,
    pub last_row_id: Option<String>,
    pub row_count: u64,
    pub algorithm: String,
    pub digest_length: u16,
    pub canonical_encoder_version: u16,
    pub row_hash_version: u16,
    pub range_hash_version: u16,
    pub segment_size: u64,
    pub digest: String,
    pub state: StoredHashState,
    pub computed_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RootHashRecord {
    pub projection_id: String,
    pub version_id: Option<String>,
    pub collection: String,
    pub schema_epoch: u64,
    pub row_count: u64,
    pub range_count: u64,
    pub algorithm: String,
    pub digest_length: u16,
    pub canonical_encoder_version: u16,
    pub row_hash_version: u16,
    pub range_hash_version: u16,
    pub root_hash_version: u16,
    pub digest: String,
    pub state: StoredHashState,
    pub computed_ms: u64,
}

#[derive(Debug, Clone)]
pub struct IntegrityCheckReport {
    pub state: crate::catalog::ProjectionVerificationState,
    pub checked_components: Vec<String>,
    pub skipped_components: Vec<String>,
    pub mismatch_count: u64,
    pub missing_count: u64,
    pub stale_count: u64,
    pub repairable: bool,
    pub elapsed_ms: u64,
    pub last_error: Option<String>,
}

impl Midge {
    pub fn hash_algorithm_metadata(&self) -> crate::catalog::ProjectionHashAlgorithmMeta {
        hash_algorithm_metadata()
    }

    pub fn row_hash(
        &self,
        collection: &str,
        row_id: &str,
    ) -> Result<Option<RowHashRecord>, CassieError> {
        let tx = self.begin_data_readonly_tx()?;
        let Some(raw) = tx
            .get(&Self::row_hash_key(collection, row_id))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid row hash metadata: {error}")))
    }

    pub fn list_row_hashes(&self, collection: &str) -> Result<Vec<RowHashRecord>, CassieError> {
        let entries =
            self.raw_scan_prefix(StorageFamily::Data, &Self::row_hash_prefix(collection))?;
        let mut out = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice::<RowHashRecord>(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        out.sort_by_key(|record| record.row_id.clone());
        Ok(out)
    }

    pub fn list_range_hashes(&self, collection: &str) -> Result<Vec<RangeHashRecord>, CassieError> {
        let entries =
            self.raw_scan_prefix(StorageFamily::Data, &Self::range_hash_prefix(collection))?;
        let mut out = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice::<RangeHashRecord>(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        out.sort_by_key(|record| record.range_id);
        Ok(out)
    }

    pub fn root_hash(&self, collection: &str) -> Result<Option<RootHashRecord>, CassieError> {
        let tx = self.begin_data_readonly_tx()?;
        let Some(raw) = tx
            .get(&Self::root_hash_key(collection))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid root hash metadata: {error}")))
    }

    pub fn compute_expected_row_hash(
        &self,
        collection: &str,
        row_id: &str,
        payload: &serde_json::Value,
    ) -> Result<RowHashRecord, CassieError> {
        let row_schema = self.row_schema(collection)?;
        Ok(compute_row_hash_record(
            collection,
            collection,
            None,
            &row_schema,
            row_id,
            payload,
        ))
    }

    pub fn rebuild_projection_hashes(
        &self,
        collection: &str,
    ) -> Result<RootHashRecord, CassieError> {
        let row_schema = self.row_schema(collection)?;
        let documents = self.scan_documents(collection)?;
        let mut live_ids = BTreeSet::new();
        let mut records = Vec::with_capacity(documents.len());
        for document in documents {
            live_ids.insert(document.id.clone());
            records.push(compute_row_hash_record(
                collection,
                collection,
                None,
                &row_schema,
                &document.id,
                &document.payload,
            ));
        }
        records.sort_by_key(|record| record.row_id.clone());

        let ranges =
            build_range_hash_records(collection, None, row_schema.schema_version, &records);
        let root = build_root_hash_record(
            collection,
            None,
            row_schema.schema_version,
            &records,
            &ranges,
        );

        let mut tx = self.begin_data_rw_tx()?;
        let existing =
            self.raw_scan_prefix(StorageFamily::Data, &Self::row_hash_prefix(collection))?;
        for (key, _value) in existing {
            let Ok(record) = serde_json::from_slice::<RowHashRecord>(&_value) else {
                continue;
            };
            if !live_ids.contains(&record.row_id) {
                tx.delete(key).map_err(CassieError::from)?;
            }
        }
        delete_keys_with_prefix_from_tx(&mut tx, Self::range_hash_prefix(collection))?;
        for record in &records {
            write_row_hash_record_to_tx(&mut tx, record)?;
        }
        for record in &ranges {
            write_range_hash_record_to_tx(&mut tx, record)?;
        }
        write_root_hash_record_to_tx(&mut tx, &root)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;

        self.update_projection_hash_metadata(collection, &records, &ranges, &root)?;
        Ok(root)
    }

    pub(crate) fn write_fresh_projection_output_rows(
        &self,
        collection: &str,
        documents: Vec<(String, serde_json::Value)>,
    ) -> Result<(super::documents::DocumentWriteBatchReport, RootHashRecord), CassieError> {
        let row_schema = self.row_schema(collection)?;
        let mut tx = self.begin_data_rw_tx()?;
        let mut report = super::documents::DocumentWriteBatchReport::default();
        let mut records = Vec::with_capacity(documents.len());

        for (id, payload) in documents {
            let row_blob = encode_row(&row_schema, &payload)?;
            tx.put(Self::row_key(collection, &id), row_blob, None)
                .map_err(CassieError::from)?;
            let record =
                compute_row_hash_record(collection, collection, None, &row_schema, &id, &payload);
            write_row_hash_record_to_tx(&mut tx, &record)?;

            report.ids.push(id);
            report.row_delta = report.row_delta.saturating_add(1);
            report.stats.row_puts = report.stats.row_puts.saturating_add(1);
            report.stats.metadata_puts = report.stats.metadata_puts.saturating_add(1);
            records.push(record);
        }

        records.sort_by_key(|record| record.row_id.clone());
        let ranges =
            build_range_hash_records(collection, None, row_schema.schema_version, &records);
        let root = build_root_hash_record(
            collection,
            None,
            row_schema.schema_version,
            &records,
            &ranges,
        );

        for record in &ranges {
            write_range_hash_record_to_tx(&mut tx, record)?;
        }
        write_root_hash_record_to_tx(&mut tx, &root)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        report.stats.batch_flushes = report.stats.batch_flushes.saturating_add(1);

        self.update_projection_hash_metadata(collection, &records, &ranges, &root)?;
        Ok((report, root))
    }

    pub fn refresh_projection_hashes_after_write(
        &self,
        collection: &str,
        row_delta: i64,
    ) -> Result<(), CassieError> {
        let metadata = self.projection_metadata(collection)?;
        let current_rows = metadata
            .as_ref()
            .map(|metadata| metadata.hashes.rows.row_count as i64)
            .unwrap_or_default()
            .saturating_add(row_delta)
            .max(0) as u64;
        if current_rows <= EAGER_HASH_REBUILD_ROW_LIMIT {
            self.rebuild_projection_hashes(collection)?;
        } else {
            self.mark_projection_hashes_stale(collection, current_rows)?;
        }
        Ok(())
    }

    pub fn projection_hash_summary(
        &self,
        collection: &str,
    ) -> Result<Option<crate::catalog::ProjectionHashMeta>, CassieError> {
        let Some(root) = self.root_hash(collection)? else {
            return Ok(None);
        };
        let row_count = root.row_count;
        let range_count = root.range_count;
        Ok(Some(crate::catalog::ProjectionHashMeta {
            algorithm: hash_algorithm_metadata(),
            rows: crate::catalog::ProjectionHashCoverageMeta {
                state: root.state.as_projection_state(),
                row_count,
                range_count: 0,
                source_checkpoint: None,
                projection_version_id: root.version_id.clone(),
                last_computed_ms: Some(root.computed_ms),
                digest: None,
                last_error: None,
            },
            ranges: crate::catalog::ProjectionHashCoverageMeta {
                state: root.state.as_projection_state(),
                row_count,
                range_count,
                source_checkpoint: None,
                projection_version_id: root.version_id.clone(),
                last_computed_ms: Some(root.computed_ms),
                digest: None,
                last_error: None,
            },
            root: crate::catalog::ProjectionHashCoverageMeta {
                state: root.state.as_projection_state(),
                row_count,
                range_count,
                source_checkpoint: None,
                projection_version_id: root.version_id,
                last_computed_ms: Some(root.computed_ms),
                digest: Some(root.digest),
                last_error: None,
            },
        }))
    }

    pub fn verify_projection_integrity(
        &self,
        collection: &str,
        hashes: bool,
        indexes: bool,
        metadata: bool,
    ) -> Result<IntegrityCheckReport, CassieError> {
        let started = std::time::Instant::now();
        let mut checked_components = Vec::new();
        let mut skipped_components = Vec::new();
        let mut mismatch_count = 0_u64;
        let mut missing_count = 0_u64;
        let mut stale_count = 0_u64;
        let mut last_error = None;

        if metadata {
            checked_components.push("metadata".to_string());
            if self.projection_metadata(collection)?.is_none() {
                missing_count += 1;
                last_error = Some("projection metadata is missing".to_string());
            }
        } else {
            skipped_components.push("metadata".to_string());
        }

        if hashes {
            checked_components.push("hashes".to_string());
            let row_schema = self.row_schema(collection)?;
            let documents = self.scan_documents(collection)?;
            let stored = self
                .list_row_hashes(collection)?
                .into_iter()
                .map(|record| (record.row_id.clone(), record))
                .collect::<BTreeMap<_, _>>();
            for document in &documents {
                let expected = compute_row_hash_record(
                    collection,
                    collection,
                    None,
                    &row_schema,
                    &document.id,
                    &document.payload,
                );
                match stored.get(&document.id) {
                    Some(actual) if actual.digest == expected.digest => {
                        if actual.state != StoredHashState::Current {
                            stale_count += 1;
                        }
                    }
                    Some(_) => mismatch_count += 1,
                    None => missing_count += 1,
                }
            }
            let ranges = self.list_range_hashes(collection)?;
            let root = self.root_hash(collection)?;
            if documents.is_empty() {
                if root.is_none() {
                    missing_count += 1;
                }
            } else {
                if ranges.is_empty() {
                    missing_count += 1;
                }
                let expected_ranges = build_range_hash_records(
                    collection,
                    None,
                    row_schema.schema_version,
                    &stored.values().cloned().collect::<Vec<_>>(),
                );
                let expected_root = build_root_hash_record(
                    collection,
                    None,
                    row_schema.schema_version,
                    &stored.values().cloned().collect::<Vec<_>>(),
                    &expected_ranges,
                );
                match root {
                    Some(actual) if actual.digest == expected_root.digest => {}
                    Some(_) => mismatch_count += 1,
                    None => missing_count += 1,
                }
            }
        } else {
            skipped_components.push("hashes".to_string());
        }

        if indexes {
            checked_components.push("indexes".to_string());
            let indexes = self.list_indexes()?;
            let vector_indexes = self.list_vector_indexes()?;
            if indexes.iter().all(|index| index.collection != collection)
                && vector_indexes
                    .iter()
                    .all(|index| index.collection != collection)
            {
                skipped_components.push("index_entries".to_string());
            }
        } else {
            skipped_components.push("indexes".to_string());
        }

        let failed = mismatch_count > 0 || missing_count > 0 || stale_count > 0;
        Ok(IntegrityCheckReport {
            state: if failed {
                crate::catalog::ProjectionVerificationState::Failed
            } else {
                crate::catalog::ProjectionVerificationState::Verified
            },
            checked_components,
            skipped_components,
            mismatch_count,
            missing_count,
            stale_count,
            repairable: missing_count > 0 || stale_count > 0,
            elapsed_ms: duration_ms(started.elapsed()),
            last_error,
        })
    }

    pub(crate) fn write_document_hash_to_tx(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        row_id: &str,
        row_schema: &RowSchema,
        payload: &serde_json::Value,
    ) -> Result<(), CassieError> {
        let record =
            compute_row_hash_record(collection, collection, None, row_schema, row_id, payload);
        write_row_hash_record_to_tx(tx, &record)
    }

    pub(crate) fn delete_document_hash_to_tx(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        row_id: &str,
    ) -> Result<(), CassieError> {
        tx.delete(Self::row_hash_key(collection, row_id))
            .map_err(CassieError::from)?;
        Ok(())
    }

    fn update_projection_hash_metadata(
        &self,
        collection: &str,
        rows: &[RowHashRecord],
        ranges: &[RangeHashRecord],
        root: &RootHashRecord,
    ) -> Result<(), CassieError> {
        let mut metadata = self
            .projection_metadata(collection)?
            .unwrap_or_else(|| ProjectionMeta::new(collection, root.schema_epoch as u32));
        let state = root.state.as_projection_state();
        let computed_ms = Some(root.computed_ms);
        metadata.hashes = crate::catalog::ProjectionHashMeta {
            algorithm: hash_algorithm_metadata(),
            rows: crate::catalog::ProjectionHashCoverageMeta {
                state: if rows.is_empty() {
                    crate::catalog::ProjectionVerificationState::Empty
                } else {
                    state.clone()
                },
                row_count: rows.len() as u64,
                range_count: 0,
                source_checkpoint: metadata.source_checkpoint.clone(),
                projection_version_id: metadata.active_version.clone(),
                last_computed_ms: computed_ms,
                digest: None,
                last_error: None,
            },
            ranges: crate::catalog::ProjectionHashCoverageMeta {
                state: if ranges.is_empty() {
                    crate::catalog::ProjectionVerificationState::Empty
                } else {
                    state.clone()
                },
                row_count: rows.len() as u64,
                range_count: ranges.len() as u64,
                source_checkpoint: metadata.source_checkpoint.clone(),
                projection_version_id: metadata.active_version.clone(),
                last_computed_ms: computed_ms,
                digest: None,
                last_error: None,
            },
            root: crate::catalog::ProjectionHashCoverageMeta {
                state,
                row_count: root.row_count,
                range_count: root.range_count,
                source_checkpoint: metadata.source_checkpoint.clone(),
                projection_version_id: metadata.active_version.clone(),
                last_computed_ms: computed_ms,
                digest: Some(root.digest.clone()),
                last_error: None,
            },
        };
        self.put_projection_metadata(metadata)
    }

    fn mark_projection_hashes_stale(
        &self,
        collection: &str,
        row_count: u64,
    ) -> Result<(), CassieError> {
        let mut metadata = self
            .projection_metadata(collection)?
            .unwrap_or_else(|| ProjectionMeta::new(collection, 1));
        metadata.hashes.algorithm = hash_algorithm_metadata();
        metadata.hashes.rows.state = if row_count == 0 {
            crate::catalog::ProjectionVerificationState::Empty
        } else {
            crate::catalog::ProjectionVerificationState::Current
        };
        metadata.hashes.rows.row_count = row_count;
        metadata.hashes.rows.last_computed_ms = Some(now_ms());
        metadata.hashes.ranges.state = crate::catalog::ProjectionVerificationState::Stale;
        metadata.hashes.ranges.row_count = row_count;
        metadata.hashes.root.state = crate::catalog::ProjectionVerificationState::Stale;
        metadata.hashes.root.row_count = row_count;
        metadata.hashes.root.last_computed_ms = Some(now_ms());
        self.put_projection_metadata(metadata)
    }
}

fn hash_algorithm_metadata() -> crate::catalog::ProjectionHashAlgorithmMeta {
    crate::catalog::ProjectionHashAlgorithmMeta {
        algorithm: ROW_HASH_ALGORITHM.to_string(),
        digest_length: ROW_HASH_DIGEST_LENGTH,
        canonical_encoder_version: CANONICAL_ENCODER_VERSION,
        hash_version: ROW_HASH_VERSION,
    }
}

fn compute_row_hash_record(
    collection: &str,
    projection_id: &str,
    version_id: Option<&str>,
    row_schema: &RowSchema,
    row_id: &str,
    payload: &serde_json::Value,
) -> RowHashRecord {
    let mut input = Vec::new();
    write_str("row", &mut input);
    write_str(projection_id, &mut input);
    write_option_str(version_id, &mut input);
    write_str(collection, &mut input);
    write_u64(row_schema.schema_version as u64, &mut input);
    write_str(row_id, &mut input);

    let object = payload.as_object();
    for field in row_schema.active_fields_by_id() {
        write_u64(field.field_id as u64, &mut input);
        write_str(&field.data_type.type_name(), &mut input);
        match object.and_then(|object| object.get(&field.name)) {
            Some(value) if value.is_null() => input.push(b'N'),
            Some(value) => {
                input.push(b'V');
                write_json_canonical(value, &mut input);
            }
            None => input.push(b'M'),
        }
    }

    RowHashRecord {
        projection_id: projection_id.to_string(),
        version_id: version_id.map(str::to_string),
        collection: collection.to_string(),
        schema_epoch: row_schema.schema_version as u64,
        row_id: row_id.to_string(),
        algorithm: ROW_HASH_ALGORITHM.to_string(),
        digest_length: ROW_HASH_DIGEST_LENGTH,
        canonical_encoder_version: CANONICAL_ENCODER_VERSION,
        row_hash_version: ROW_HASH_VERSION,
        digest: digest_hex(&input),
        state: StoredHashState::Current,
        computed_ms: now_ms(),
    }
}

fn build_range_hash_records(
    collection: &str,
    version_id: Option<String>,
    schema_epoch: u32,
    rows: &[RowHashRecord],
) -> Vec<RangeHashRecord> {
    rows.chunks(RANGE_SEGMENT_SIZE)
        .enumerate()
        .map(|(index, chunk)| {
            let mut input = Vec::new();
            write_str("range", &mut input);
            write_str(collection, &mut input);
            write_option_str(version_id.as_deref(), &mut input);
            write_u64(schema_epoch as u64, &mut input);
            write_u64(index as u64, &mut input);
            write_u64(RANGE_SEGMENT_SIZE as u64, &mut input);
            for row in chunk {
                write_str(&row.row_id, &mut input);
                write_str(&row.digest, &mut input);
            }
            RangeHashRecord {
                projection_id: collection.to_string(),
                version_id: version_id.clone(),
                collection: collection.to_string(),
                schema_epoch: schema_epoch as u64,
                range_id: index as u64,
                first_row_id: chunk.first().map(|record| record.row_id.clone()),
                last_row_id: chunk.last().map(|record| record.row_id.clone()),
                row_count: chunk.len() as u64,
                algorithm: ROW_HASH_ALGORITHM.to_string(),
                digest_length: ROW_HASH_DIGEST_LENGTH,
                canonical_encoder_version: CANONICAL_ENCODER_VERSION,
                row_hash_version: ROW_HASH_VERSION,
                range_hash_version: RANGE_HASH_VERSION,
                segment_size: RANGE_SEGMENT_SIZE as u64,
                digest: digest_hex(&input),
                state: StoredHashState::Current,
                computed_ms: now_ms(),
            }
        })
        .collect()
}

fn build_root_hash_record(
    collection: &str,
    version_id: Option<String>,
    schema_epoch: u32,
    rows: &[RowHashRecord],
    ranges: &[RangeHashRecord],
) -> RootHashRecord {
    let mut input = Vec::new();
    write_str("root", &mut input);
    write_str(collection, &mut input);
    write_option_str(version_id.as_deref(), &mut input);
    write_u64(schema_epoch as u64, &mut input);
    write_u64(rows.len() as u64, &mut input);
    write_u64(ranges.len() as u64, &mut input);
    if ranges.is_empty() {
        write_str("empty", &mut input);
    }
    for range in ranges {
        write_u64(range.range_id, &mut input);
        write_str(&range.digest, &mut input);
    }
    RootHashRecord {
        projection_id: collection.to_string(),
        version_id,
        collection: collection.to_string(),
        schema_epoch: schema_epoch as u64,
        row_count: rows.len() as u64,
        range_count: ranges.len() as u64,
        algorithm: ROW_HASH_ALGORITHM.to_string(),
        digest_length: ROW_HASH_DIGEST_LENGTH,
        canonical_encoder_version: CANONICAL_ENCODER_VERSION,
        row_hash_version: ROW_HASH_VERSION,
        range_hash_version: RANGE_HASH_VERSION,
        root_hash_version: ROOT_HASH_VERSION,
        digest: digest_hex(&input),
        state: if rows.is_empty() {
            StoredHashState::Empty
        } else {
            StoredHashState::Current
        },
        computed_ms: now_ms(),
    }
}

fn write_row_hash_record_to_tx(
    tx: &mut cntryl_midge::Transaction,
    record: &RowHashRecord,
) -> Result<(), CassieError> {
    tx.put(
        Midge::row_hash_key(&record.collection, &record.row_id),
        serde_json::to_vec(record).map_err(|error| CassieError::Parse(error.to_string()))?,
        None,
    )
    .map_err(CassieError::from)?;
    Ok(())
}

fn write_range_hash_record_to_tx(
    tx: &mut cntryl_midge::Transaction,
    record: &RangeHashRecord,
) -> Result<(), CassieError> {
    tx.put(
        Midge::range_hash_key(&record.collection, record.range_id),
        serde_json::to_vec(record).map_err(|error| CassieError::Parse(error.to_string()))?,
        None,
    )
    .map_err(CassieError::from)?;
    Ok(())
}

fn write_root_hash_record_to_tx(
    tx: &mut cntryl_midge::Transaction,
    record: &RootHashRecord,
) -> Result<(), CassieError> {
    tx.put(
        Midge::root_hash_key(&record.collection),
        serde_json::to_vec(record).map_err(|error| CassieError::Parse(error.to_string()))?,
        None,
    )
    .map_err(CassieError::from)?;
    Ok(())
}

fn delete_keys_with_prefix_from_tx(
    tx: &mut cntryl_midge::Transaction,
    prefix: Vec<u8>,
) -> Result<(), CassieError> {
    let mut scan = tx
        .scan(&Query::new().prefix(prefix.into()))
        .map_err(CassieError::from)?;
    let mut keys = Vec::new();
    while let Some((key, _value)) = scan.next() {
        keys.push(key);
    }
    drop(scan);
    for key in keys {
        tx.delete(key).map_err(CassieError::from)?;
    }
    Ok(())
}

fn write_json_canonical(value: &serde_json::Value, out: &mut Vec<u8>) {
    match value {
        serde_json::Value::Null => out.push(b'0'),
        serde_json::Value::Bool(value) => {
            out.push(b'b');
            out.push(u8::from(*value));
        }
        serde_json::Value::Number(value) => {
            out.push(b'n');
            write_str(&value.to_string(), out);
        }
        serde_json::Value::String(value) => {
            out.push(b's');
            write_str(value, out);
        }
        serde_json::Value::Array(values) => {
            out.push(b'a');
            write_u64(values.len() as u64, out);
            for value in values {
                write_json_canonical(value, out);
            }
        }
        serde_json::Value::Object(values) => {
            out.push(b'o');
            write_u64(values.len() as u64, out);
            let mut entries = values.iter().collect::<Vec<_>>();
            entries.sort_by_key(|(key, _)| *key);
            for (key, value) in entries {
                write_str(key, out);
                write_json_canonical(value, out);
            }
        }
    }
}

fn digest_hex(input: &[u8]) -> String {
    let mut left = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d_u128;
    let mut right = 0x0000_0000_0100_0000_0000_0000_0000_013b_u128;
    for byte in input {
        left ^= u128::from(*byte);
        left = left.wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b_u128);
        right ^= u128::from(byte.rotate_left(1));
        right = right.wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_0159_u128);
    }
    format!("{:032x}", left ^ right.rotate_left(17))
}

fn write_str(value: &str, out: &mut Vec<u8>) {
    write_u64(value.len() as u64, out);
    out.extend_from_slice(value.as_bytes());
}

fn write_option_str(value: Option<&str>, out: &mut Vec<u8>) {
    match value {
        Some(value) => {
            out.push(1);
            write_str(value, out);
        }
        None => out.push(0),
    }
}

fn write_u64(value: u64, out: &mut Vec<u8>) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn duration_ms(duration: std::time::Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}
