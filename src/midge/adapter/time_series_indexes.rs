use cntryl_midge::{Query, WriteOptions};
use time::{format_description::well_known::Rfc3339, Duration as TimeDuration, OffsetDateTime};

use super::{
    check_document_write_failure_point, collect_scan, decode_projected_row, decode_row, encode_row,
    key_encoding, CassieError, DocumentRef, DocumentWriteFailurePoint, Midge, RowDecode, Uuid,
};
use crate::catalog::{IndexKind, IndexMeta};

#[path = "time_series_indexes/metadata.rs"]
mod metadata;
use metadata::{BucketIdentity, TimeSeriesManifest};

pub(crate) const INCOMPLETE_BUCKET_MEMBERSHIP: &str = "incomplete-bucket-membership";
pub(crate) const DANGLING_BUCKET_MEMBERSHIP: &str = "dangling-bucket-membership";
pub(crate) const MISSING_BUCKET_METADATA: &str = "missing-bucket-metadata";
pub(crate) const STALE_BUCKET_METADATA: &str = "stale-bucket-metadata";
pub(crate) const CORRUPT_BUCKET_METADATA: &str = "corrupt-bucket-metadata";

#[derive(Debug, Clone)]
pub(crate) struct TimeSeriesIndexScanHit {
    pub id: String,
    pub bucket_key: String,
    pub timestamp: String,
    entry_key: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TimeSeriesIndexScanReport {
    pub hits: Vec<TimeSeriesIndexScanHit>,
    pub entries_scanned: usize,
    pub generation: u64,
}

#[derive(Debug)]
pub(crate) enum TimeSeriesIndexScanOutcome {
    Native(TimeSeriesIndexScanReport),
    Fallback(&'static str),
}

#[derive(Debug)]
pub(crate) enum TimeSeriesDocumentScanOutcome {
    Native(Vec<DocumentRef>),
    Fallback(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TimeSeriesIndexEntry {
    key: Vec<u8>,
    bucket: BucketIdentity,
}

struct DecodedTimeSeriesHits {
    hits: Vec<TimeSeriesIndexScanHit>,
    observed_counts: std::collections::BTreeMap<BucketIdentity, u64>,
}

struct TimeSeriesIntegrity {
    generation: u64,
    manifest_total: u64,
    expected_total: u64,
    expected_counts: std::collections::BTreeMap<BucketIdentity, u64>,
}

enum TimeSeriesIntegrityOutcome {
    Valid(TimeSeriesIntegrity),
    Fallback(&'static str),
}

fn decode_time_series_hits(
    scan: Vec<(Vec<u8>, Vec<u8>)>,
    data_prefix: &[u8],
    index_name: &str,
) -> Result<DecodedTimeSeriesHits, CassieError> {
    let mut seen_ids = std::collections::BTreeSet::new();
    let mut observed_counts = std::collections::BTreeMap::<BucketIdentity, u64>::new();
    let mut hits = Vec::new();
    for (key, raw_value) in scan {
        if !raw_value.is_empty() {
            return Err(CassieError::Parse(
                "time-series index values must be empty".to_string(),
            ));
        }
        let (partition, bucket_seconds, timestamp_seconds, timestamp_nanos, id) =
            key_encoding::decode_time_series_entry_key(&key, data_prefix).ok_or_else(|| {
                CassieError::Parse(format!("invalid time-series index key for '{index_name}'"))
            })?;
        let bucket_start = OffsetDateTime::from_unix_timestamp(bucket_seconds)
            .map_err(|error| CassieError::Parse(format!("invalid bucket time: {error}")))?
            .format(&Rfc3339)
            .map_err(|error| CassieError::Parse(format!("invalid bucket format: {error}")))?;
        let timestamp = OffsetDateTime::from_unix_timestamp(timestamp_seconds)
            .and_then(|value| value.replace_nanosecond(timestamp_nanos))
            .map_err(|error| CassieError::Parse(format!("invalid timestamp: {error}")))?
            .format(&Rfc3339)
            .map_err(|error| CassieError::Parse(format!("invalid timestamp format: {error}")))?;
        if !seen_ids.insert(id.clone()) {
            return Err(CassieError::Parse(
                "duplicate time-series membership id".to_string(),
            ));
        }
        let bucket = BucketIdentity {
            partition: partition.clone(),
            start_seconds: bucket_seconds,
        };
        let count = observed_counts.entry(bucket).or_default();
        *count = count.saturating_add(1);
        hits.push(TimeSeriesIndexScanHit {
            id,
            bucket_key: format!("{partition}\t{bucket_start}"),
            timestamp,
            entry_key: key,
        });
    }
    Ok(DecodedTimeSeriesHits {
        hits,
        observed_counts,
    })
}

fn time_series_scan_query(
    relation_id: u64,
    index_id: u64,
    partition_key: Option<&str>,
    lower_bucket_seconds: Option<i64>,
    upper_bucket_seconds: Option<i64>,
) -> Query {
    let Some(partition_key) = partition_key else {
        return Query::new()
            .prefix(key_encoding::time_series_index_data_prefix(relation_id, index_id).into());
    };
    let prefix =
        key_encoding::time_series_index_partition_prefix(relation_id, index_id, partition_key);
    let mut query = Query::new().prefix(prefix.into());
    if let Some(lower) = lower_bucket_seconds {
        query = query.start_key(
            key_encoding::time_series_index_bucket_bound_key(
                relation_id,
                index_id,
                partition_key,
                lower,
            )
            .into(),
        );
    }
    if let Some(upper) = upper_bucket_seconds {
        query = query.end_key(
            key_encoding::time_series_index_bucket_bound_key(
                relation_id,
                index_id,
                partition_key,
                upper,
            )
            .into(),
        );
    }
    query
}

impl Midge {
    pub(crate) fn reconcile_time_series_indexes(&self) -> Result<(), CassieError> {
        let indexes = self
            .list_indexes()?
            .into_iter()
            .filter(Self::time_series_index_supports_storage)
            .collect::<Vec<_>>();
        for index in indexes {
            let valid = match self.scan_time_series_index(&index, None, None, None)? {
                TimeSeriesIndexScanOutcome::Native(report) => matches!(
                    self.scan_time_series_hit_documents(&index, &report.hits, &[])?,
                    TimeSeriesDocumentScanOutcome::Native(_)
                ),
                TimeSeriesIndexScanOutcome::Fallback(_) => false,
            };
            if !valid {
                self.rebuild_time_series_index_for_index(&index)?;
            }
        }
        Ok(())
    }

    /// Load documents for a newly-created time-series fixture collection.
    ///
    /// This skips replacement checks and non-time-series secondary-index maintenance; callers must
    /// only use it for fresh row-store collections whose secondary indexes are time-series indexes.
    ///
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_fresh_time_series_documents(
        &self,
        collection: &str,
        documents: Vec<(Option<String>, serde_json::Value)>,
    ) -> Result<Vec<String>, CassieError> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }
        let collection = self.canonical_collection_name(collection);
        if self.collection_uses_column_store(&collection)? {
            return Err(CassieError::Unsupported(
                "fresh time-series document load requires row storage".to_string(),
            ));
        }
        if self
            .list_vector_indexes_canonical()?
            .iter()
            .any(|index| index.collection.eq_ignore_ascii_case(&collection))
        {
            return Err(CassieError::Unsupported(
                "fresh time-series document load does not maintain vector indexes".to_string(),
            ));
        }

        let indexes = self
            .list_indexes()?
            .into_iter()
            .filter(|index| index.collection.eq_ignore_ascii_case(&collection))
            .collect::<Vec<_>>();
        if indexes
            .iter()
            .any(|index| index.kind != IndexKind::TimeSeries)
        {
            return Err(CassieError::Unsupported(
                "fresh time-series document load only maintains time-series indexes".to_string(),
            ));
        }

        let schema = self
            .collection_schema(&collection)
            .ok_or_else(|| CassieError::CollectionNotFound(collection.clone()))?;
        let row_schema = self.row_schema(&collection)?;
        let write_gate = self.collection_write_gate(&collection);
        let _write_guard = write_gate.lock();
        let generation = self.collection_generation(&collection)?.saturating_add(1);
        let mut tx = self.begin_data_rw_tx_for(&collection)?;
        let mut ids = Vec::with_capacity(documents.len());

        for (id, payload) in documents {
            Self::validate_document(&schema, &payload)?;
            let id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
            let row_blob = encode_row(&row_schema, &payload)?;
            tx.put(Self::row_key(row_schema.relation_id, &id), row_blob, None)
                .map_err(CassieError::from)?;
            Self::write_document_hash_to_tx(&mut tx, &collection, &id, &row_schema, &payload)?;
            Self::sync_time_series_indexes_for_document(
                &mut tx,
                &id,
                None,
                Some(&payload),
                &indexes,
                generation,
            )?;
            ids.push(id);
        }

        let row_delta = i64::try_from(ids.len()).unwrap_or(i64::MAX);
        let generation = Self::increment_collection_generation_in_tx(&mut tx, &collection)?;
        Self::record_column_batch_maintenance_debt_in_tx(&mut tx, &collection, generation)?;
        Self::record_projection_hash_maintenance_debt_in_tx(&mut tx, &collection, generation)?;
        Self::increment_data_epoch_in_tx(&mut tx)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        let _ = self.complete_column_batch_maintenance(&collection, generation);
        let _ = self.complete_projection_hash_maintenance(&collection, generation, row_delta);
        Ok(ids)
    }

    pub(crate) fn sync_time_series_indexes_for_document(
        tx: &mut cntryl_midge::Transaction,
        id: &str,
        old_payload: Option<&serde_json::Value>,
        new_payload: Option<&serde_json::Value>,
        indexes: &[IndexMeta],
        generation: u64,
    ) -> Result<(usize, usize), CassieError> {
        let mut deletes = 0usize;
        let mut puts = 0usize;

        for index in indexes {
            let old_entry = match old_payload {
                Some(payload) => Self::time_series_index_entry(index, id, payload)?,
                None => None,
            };
            let new_entry = match new_payload {
                Some(payload) => Self::time_series_index_entry(index, id, payload)?,
                None => None,
            };

            match (old_entry.as_ref(), new_entry.as_ref()) {
                (Some(old), Some(new)) if old.key == new.key => {}
                _ => {
                    if let Some(old) = old_entry.as_ref() {
                        tx.delete(old.key.clone()).map_err(CassieError::from)?;
                        deletes += 1;
                    }
                    if let Some(new) = new_entry.as_ref() {
                        tx.put(new.key.clone(), Vec::new(), None)
                            .map_err(CassieError::from)?;
                        puts += 1;
                    }
                }
            }
            Self::update_time_series_metadata_in_tx(
                tx,
                index,
                old_entry.as_ref(),
                new_entry.as_ref(),
                generation,
            )?;
        }

        check_document_write_failure_point(DocumentWriteFailurePoint::TimeSeriesIndex)?;

        Ok((deletes, puts))
    }

    fn update_time_series_metadata_in_tx(
        tx: &mut cntryl_midge::Transaction,
        index: &IndexMeta,
        old_entry: Option<&TimeSeriesIndexEntry>,
        new_entry: Option<&TimeSeriesIndexEntry>,
        generation: u64,
    ) -> Result<(), CassieError> {
        let (relation_id, index_id) = Self::time_series_storage_ids(index)?;
        let manifest_key = key_encoding::time_series_index_manifest_key(relation_id, index_id);
        let mut manifest = tx
            .get(&manifest_key)
            .map_err(CassieError::from)?
            .map_or_else(
                || Ok(TimeSeriesManifest::empty(generation)),
                |raw| metadata::decode_manifest(&raw),
            )?;
        if manifest.version != metadata::FORMAT_VERSION {
            return Err(CassieError::Parse(format!(
                "unsupported time-series manifest version {}",
                manifest.version
            )));
        }

        if old_entry.map(|entry| &entry.key) != new_entry.map(|entry| &entry.key) {
            if let Some(old) = old_entry {
                Self::adjust_time_series_bucket_count_in_tx(
                    tx,
                    relation_id,
                    index_id,
                    &old.bucket,
                    -1,
                )?;
                manifest.total_membership =
                    manifest.total_membership.checked_sub(1).ok_or_else(|| {
                        CassieError::Parse("time-series manifest membership underflow".to_string())
                    })?;
            }
            if let Some(new) = new_entry {
                Self::adjust_time_series_bucket_count_in_tx(
                    tx,
                    relation_id,
                    index_id,
                    &new.bucket,
                    1,
                )?;
                manifest.total_membership =
                    manifest.total_membership.checked_add(1).ok_or_else(|| {
                        CassieError::ResourceLimit(
                            "time-series manifest membership overflow".to_string(),
                        )
                    })?;
            }
        }
        manifest.generation = generation;
        tx.put(manifest_key, metadata::encode_manifest(&manifest)?, None)
            .map_err(CassieError::from)
    }

    fn adjust_time_series_bucket_count_in_tx(
        tx: &mut cntryl_midge::Transaction,
        relation_id: u64,
        index_id: u64,
        bucket: &BucketIdentity,
        delta: i8,
    ) -> Result<(), CassieError> {
        let key = key_encoding::time_series_index_bucket_count_key(
            relation_id,
            index_id,
            &bucket.partition,
            bucket.start_seconds,
        );
        let current = tx
            .get(&key)
            .map_err(CassieError::from)?
            .map_or(Ok(0), |raw| metadata::decode_count(&raw))?;
        let next = match delta {
            -1 => current.checked_sub(1).ok_or_else(|| {
                CassieError::Parse("time-series bucket count underflow".to_string())
            })?,
            1 => current.checked_add(1).ok_or_else(|| {
                CassieError::ResourceLimit("time-series bucket count overflow".to_string())
            })?,
            _ => {
                return Err(CassieError::Execution(
                    "invalid time-series bucket count delta".to_string(),
                ));
            }
        };
        if next == 0 {
            tx.delete(key).map_err(CassieError::from)
        } else {
            tx.put(key, metadata::encode_count(next).to_vec(), None)
                .map_err(CassieError::from)
        }
    }

    pub(crate) fn rebuild_time_series_indexes_for_collection(
        &self,
        collection: &str,
    ) -> Result<(), CassieError> {
        for index in self.list_indexes()?.into_iter().filter(|index| {
            index.collection == collection && Self::time_series_index_supports_storage(index)
        }) {
            self.rebuild_time_series_index_for_index(&index)?;
        }
        Ok(())
    }

    pub(crate) fn rebuild_time_series_index_for_index(
        &self,
        index: &IndexMeta,
    ) -> Result<(), CassieError> {
        if !Self::time_series_index_supports_storage(index) {
            self.delete_time_series_index_data(&index.collection, &index.name)?;
            return Ok(());
        }

        let write_gate = self.collection_write_gate(&index.collection);
        let _write_guard = write_gate.lock();
        self.rebuild_time_series_index_for_index_locked(index)
    }

    fn rebuild_time_series_index_for_index_locked(
        &self,
        index: &IndexMeta,
    ) -> Result<(), CassieError> {
        let rows = self.scan_rows_for_rebuild(&index.collection, RowDecode::Full)?;
        let generation = self.collection_generation(&index.collection)?;
        let (relation_id, index_id) = Self::time_series_storage_ids(index)?;
        let mut entries = Vec::new();
        let mut bucket_counts = std::collections::BTreeMap::<BucketIdentity, u64>::new();
        for row in rows {
            if let Some(entry) = Self::time_series_index_entry(index, &row.id, &row.payload)? {
                let count = bucket_counts.entry(entry.bucket.clone()).or_default();
                *count = count.checked_add(1).ok_or_else(|| {
                    CassieError::ResourceLimit("time-series bucket count overflow".to_string())
                })?;
                entries.push(entry);
            }
        }
        let total_membership = u64::try_from(entries.len()).map_err(|_| {
            CassieError::ResourceLimit("time-series membership count overflow".to_string())
        })?;

        let mut tx = self.begin_data_rw_tx_for(&index.collection)?;
        Self::delete_keys_with_prefix(
            &mut tx,
            key_encoding::time_series_index_artifact_prefix(relation_id, index_id),
        )?;
        for entry in entries {
            tx.put(entry.key, Vec::new(), None)
                .map_err(CassieError::from)?;
        }
        for (bucket, count) in bucket_counts {
            tx.put(
                key_encoding::time_series_index_bucket_count_key(
                    relation_id,
                    index_id,
                    &bucket.partition,
                    bucket.start_seconds,
                ),
                metadata::encode_count(count).to_vec(),
                None,
            )
            .map_err(CassieError::from)?;
        }
        tx.put(
            key_encoding::time_series_index_manifest_key(relation_id, index_id),
            metadata::encode_manifest(&TimeSeriesManifest {
                version: metadata::FORMAT_VERSION,
                generation,
                total_membership,
            })?,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)
    }

    pub(crate) fn delete_time_series_index_data(
        &self,
        collection: &str,
        index_name: &str,
    ) -> Result<(), CassieError> {
        let Some(index) = self.get_index(collection, index_name)? else {
            return Ok(());
        };
        let (relation_id, index_id) = Self::time_series_storage_ids(&index)?;
        let mut tx = self.begin_data_rw_tx_for(collection)?;
        Self::delete_keys_with_prefix(
            &mut tx,
            key_encoding::time_series_index_artifact_prefix(relation_id, index_id),
        )?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    pub(crate) fn scan_time_series_index(
        &self,
        index: &IndexMeta,
        partition_key: Option<&str>,
        lower_bucket_seconds: Option<i64>,
        upper_bucket_seconds: Option<i64>,
    ) -> Result<TimeSeriesIndexScanOutcome, CassieError> {
        let index = self.resolved_time_series_index(index)?;
        if !Self::time_series_index_supports_storage(&index) {
            return Err(CassieError::Unsupported(format!(
                "time-series index '{}' has unsupported bucket storage options",
                index.name
            )));
        }

        let tx = self.begin_data_readonly_tx_for(&index.collection)?;
        let (relation_id, index_id) = Self::time_series_storage_ids(&index)?;
        let integrity =
            match self.load_time_series_integrity(&tx, &index.collection, relation_id, index_id)? {
                TimeSeriesIntegrityOutcome::Valid(integrity) => integrity,
                TimeSeriesIntegrityOutcome::Fallback(reason) => {
                    return Ok(TimeSeriesIndexScanOutcome::Fallback(reason));
                }
            };
        let TimeSeriesIntegrity {
            generation,
            manifest_total,
            expected_total,
            expected_counts,
        } = integrity;
        let data_prefix = Self::time_series_index_data_prefix(relation_id, index_id);
        let query = time_series_scan_query(
            relation_id,
            index_id,
            partition_key,
            lower_bucket_seconds,
            upper_bucket_seconds,
        );
        let scan = collect_scan(tx.scan(&query).map_err(CassieError::from)?)?;
        let entries_scanned = scan.len();
        let decoded = decode_time_series_hits(scan, &data_prefix, &index.name)?;

        let requested_counts = expected_counts
            .iter()
            .filter(|(bucket, _)| {
                partition_key.is_none_or(|partition| partition == bucket.partition)
                    && lower_bucket_seconds.is_none_or(|lower| bucket.start_seconds >= lower)
                    && upper_bucket_seconds.is_none_or(|upper| bucket.start_seconds < upper)
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        for (bucket, expected) in requested_counts {
            let observed = decoded
                .observed_counts
                .get(bucket)
                .copied()
                .unwrap_or_default();
            if observed != *expected {
                return Ok(TimeSeriesIndexScanOutcome::Fallback(
                    INCOMPLETE_BUCKET_MEMBERSHIP,
                ));
            }
        }
        if decoded
            .observed_counts
            .keys()
            .any(|bucket| !expected_counts.contains_key(bucket))
        {
            return Ok(TimeSeriesIndexScanOutcome::Fallback(
                MISSING_BUCKET_METADATA,
            ));
        }
        if expected_total != manifest_total {
            return Ok(TimeSeriesIndexScanOutcome::Fallback(
                CORRUPT_BUCKET_METADATA,
            ));
        }

        Ok(TimeSeriesIndexScanOutcome::Native(
            TimeSeriesIndexScanReport {
                hits: decoded.hits,
                entries_scanned,
                generation,
            },
        ))
    }

    fn load_time_series_integrity(
        &self,
        tx: &cntryl_midge::Transaction,
        collection: &str,
        relation_id: u64,
        index_id: u64,
    ) -> Result<TimeSeriesIntegrityOutcome, CassieError> {
        let manifest_key = key_encoding::time_series_index_manifest_key(relation_id, index_id);
        let Some(manifest_raw) = tx.get(&manifest_key).map_err(CassieError::from)? else {
            return Ok(TimeSeriesIntegrityOutcome::Fallback(
                MISSING_BUCKET_METADATA,
            ));
        };
        let Ok(manifest) = metadata::decode_manifest(&manifest_raw) else {
            return Ok(TimeSeriesIntegrityOutcome::Fallback(
                CORRUPT_BUCKET_METADATA,
            ));
        };
        if manifest.version != metadata::FORMAT_VERSION {
            return Ok(TimeSeriesIntegrityOutcome::Fallback(
                CORRUPT_BUCKET_METADATA,
            ));
        }
        let generation = self.collection_generation(collection)?;
        if manifest.generation != generation {
            return Ok(TimeSeriesIntegrityOutcome::Fallback(STALE_BUCKET_METADATA));
        }

        let count_prefix =
            key_encoding::time_series_index_bucket_count_prefix(relation_id, index_id);
        let count_entries = collect_scan(
            tx.scan(&Query::new().prefix(count_prefix.clone().into()))
                .map_err(CassieError::from)?,
        )?;
        let mut expected_counts = std::collections::BTreeMap::new();
        let mut expected_total = 0u64;
        for (key, raw) in count_entries {
            let Some((partition, start_seconds)) =
                key_encoding::decode_time_series_bucket_count_key(&key, &count_prefix)
            else {
                return Ok(TimeSeriesIntegrityOutcome::Fallback(
                    CORRUPT_BUCKET_METADATA,
                ));
            };
            let Ok(count) = metadata::decode_count(&raw) else {
                return Ok(TimeSeriesIntegrityOutcome::Fallback(
                    CORRUPT_BUCKET_METADATA,
                ));
            };
            if count == 0
                || expected_counts
                    .insert(
                        BucketIdentity {
                            partition,
                            start_seconds,
                        },
                        count,
                    )
                    .is_some()
            {
                return Ok(TimeSeriesIntegrityOutcome::Fallback(
                    CORRUPT_BUCKET_METADATA,
                ));
            }
            let Some(total) = expected_total.checked_add(count) else {
                return Ok(TimeSeriesIntegrityOutcome::Fallback(
                    CORRUPT_BUCKET_METADATA,
                ));
            };
            expected_total = total;
        }
        Ok(TimeSeriesIntegrityOutcome::Valid(TimeSeriesIntegrity {
            generation,
            manifest_total: manifest.total_membership,
            expected_total,
            expected_counts,
        }))
    }

    pub(crate) fn scan_time_series_hit_documents(
        &self,
        index: &IndexMeta,
        hits: &[TimeSeriesIndexScanHit],
        fields: &[String],
    ) -> Result<TimeSeriesDocumentScanOutcome, CassieError> {
        if hits.is_empty() {
            return Ok(TimeSeriesDocumentScanOutcome::Native(Vec::new()));
        }
        let index = self.resolved_time_series_index(index)?;
        let collection = &index.collection;
        let row_schema = self.row_schema(collection)?;
        let projection = fields
            .iter()
            .map(|field| field.to_ascii_lowercase())
            .collect::<std::collections::HashSet<_>>();
        let tx = self.begin_data_readonly_tx_for(collection)?;
        let mut documents = Vec::with_capacity(hits.len());
        for hit in hits {
            let raw = match tx
                .get(&Self::row_key(row_schema.relation_id, &hit.id))
                .map_err(CassieError::from)?
            {
                Some(raw) => Some(raw),
                None => tx
                    .get(&Self::doc_key(collection, &hit.id))
                    .map_err(CassieError::from)?,
            };
            let Some(raw) = raw else {
                return Ok(TimeSeriesDocumentScanOutcome::Fallback(
                    DANGLING_BUCKET_MEMBERSHIP,
                ));
            };
            let full_payload = decode_row(&row_schema, &raw)?;
            let Some(expected_entry) =
                Self::time_series_index_entry(&index, &hit.id, &full_payload)?
            else {
                return Ok(TimeSeriesDocumentScanOutcome::Fallback(
                    DANGLING_BUCKET_MEMBERSHIP,
                ));
            };
            if expected_entry.key != hit.entry_key {
                return Ok(TimeSeriesDocumentScanOutcome::Fallback(
                    DANGLING_BUCKET_MEMBERSHIP,
                ));
            }
            let payload = decode_projected_row(&row_schema, &raw, &projection)?;
            documents.push(DocumentRef {
                id: hit.id.clone(),
                payload,
            });
        }

        Ok(TimeSeriesDocumentScanOutcome::Native(documents))
    }

    fn time_series_index_supports_storage(index: &IndexMeta) -> bool {
        index.kind == IndexKind::TimeSeries
            && index.normalized_fields().len() == 1
            && bucket_width_duration(index).is_some()
    }

    fn resolved_time_series_index(&self, index: &IndexMeta) -> Result<IndexMeta, CassieError> {
        let mut resolved = index.clone();
        resolved.collection = self.canonical_collection_name(&resolved.collection);
        if resolved.storage_id().is_none() || resolved.relation_id().is_none() {
            let stored = self
                .get_index(&resolved.collection, &resolved.name)?
                .ok_or_else(|| {
                    CassieError::Parse(format!("index '{}' not found", resolved.name))
                })?;
            resolved.set_storage_ids(
                stored.relation_id().ok_or_else(|| {
                    CassieError::Parse(format!(
                        "index '{}' is missing its relation id",
                        resolved.name
                    ))
                })?,
                stored.storage_id().ok_or_else(|| {
                    CassieError::Parse(format!(
                        "index '{}' is missing its storage id",
                        resolved.name
                    ))
                })?,
            );
        }
        Ok(resolved)
    }

    fn time_series_storage_ids(index: &IndexMeta) -> Result<(u64, u64), CassieError> {
        let relation_id = index.relation_id().ok_or_else(|| {
            CassieError::Parse(format!("index '{}' is missing its relation id", index.name))
        })?;
        let index_id = index.storage_id().ok_or_else(|| {
            CassieError::Parse(format!("index '{}' is missing its storage id", index.name))
        })?;
        Ok((relation_id, index_id))
    }

    fn time_series_index_entry(
        index: &IndexMeta,
        id: &str,
        payload: &serde_json::Value,
    ) -> Result<Option<TimeSeriesIndexEntry>, CassieError> {
        if !Self::time_series_index_supports_storage(index) {
            return Ok(None);
        }
        let timestamp_field = index.primary_field();
        let Some(timestamp) =
            payload_field(payload, &timestamp_field).and_then(|value| value.as_str())
        else {
            return Ok(None);
        };
        let Some(bucket_key) = bucket_key_for_timestamp(index, payload, timestamp) else {
            return Ok(None);
        };
        let Some((partition_key, bucket_start)) = bucket_key.split_once('\t') else {
            return Ok(None);
        };
        let bucket_start_seconds = OffsetDateTime::parse(bucket_start, &Rfc3339)
            .ok()
            .map(OffsetDateTime::unix_timestamp)
            .ok_or_else(|| CassieError::Parse("invalid time-series bucket bound".to_string()))?;
        let parsed_timestamp = OffsetDateTime::parse(timestamp, &Rfc3339).map_err(|error| {
            CassieError::Parse(format!("invalid time-series timestamp: {error}"))
        })?;
        let key = key_encoding::time_series_index_entry_key(
            index.relation_id().ok_or_else(|| {
                CassieError::Parse(format!("index '{}' is missing its relation id", index.name))
            })?,
            index.storage_id().ok_or_else(|| {
                CassieError::Parse(format!("index '{}' is missing its storage id", index.name))
            })?,
            partition_key,
            bucket_start_seconds,
            parsed_timestamp.unix_timestamp(),
            parsed_timestamp.nanosecond(),
            id,
        );
        Ok(Some(TimeSeriesIndexEntry {
            key,
            bucket: BucketIdentity {
                partition: partition_key.to_string(),
                start_seconds: bucket_start_seconds,
            },
        }))
    }
}

fn bucket_key_for_timestamp(
    index: &IndexMeta,
    payload: &serde_json::Value,
    timestamp: &str,
) -> Option<String> {
    let duration = bucket_width_duration(index)?;
    let timestamp = OffsetDateTime::parse(timestamp, &Rfc3339).ok()?;
    let width_ns = duration.whole_nanoseconds();
    if width_ns <= 0 {
        return None;
    }
    let timestamp_ns = timestamp.unix_timestamp_nanos();
    let bucket_ns = timestamp_ns.div_euclid(width_ns).checked_mul(width_ns)?;
    let bucket_start = OffsetDateTime::from_unix_timestamp_nanos(bucket_ns)
        .ok()?
        .format(&Rfc3339)
        .ok()?;
    let partition = partition_key(index, payload);
    Some(format!("{partition}\t{bucket_start}"))
}

fn bucket_width_duration(index: &IndexMeta) -> Option<TimeDuration> {
    let raw = index.options.get("bucket_width")?.trim();
    let mut parts = raw.split_whitespace();
    let amount = parts.next()?.parse::<i64>().ok()?;
    let unit = parts.next()?.to_ascii_lowercase();
    if amount <= 0 || parts.next().is_some() {
        return None;
    }
    match unit.as_str() {
        "minute" | "minutes" => Some(TimeDuration::minutes(amount)),
        "hour" | "hours" => Some(TimeDuration::hours(amount)),
        "day" | "days" => Some(TimeDuration::days(amount)),
        _ => None,
    }
}

fn partition_key(index: &IndexMeta, payload: &serde_json::Value) -> String {
    let Some(raw) = index.options.get("partition_by") else {
        return "none".to_string();
    };
    let values = raw
        .split(',')
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .map(|field| {
            payload_field(payload, field).map_or_else(|| "null".to_string(), partition_value)
        })
        .collect::<Vec<_>>();
    if values.is_empty() {
        "none".to_string()
    } else {
        values.join("\u{1f}")
    }
}

fn partition_value(value: &serde_json::Value) -> String {
    if let Some(value) = value.as_str() {
        return value.to_string();
    }
    if value.is_null() {
        return "null".to_string();
    }
    value.to_string()
}

fn payload_field<'a>(payload: &'a serde_json::Value, field: &str) -> Option<&'a serde_json::Value> {
    payload.as_object().and_then(|object| {
        object.get(field).or_else(|| {
            object
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(field))
                .map(|(_, value)| value)
        })
    })
}
