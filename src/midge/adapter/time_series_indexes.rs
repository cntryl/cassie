use cntryl_midge::{Query, WriteOptions};
use serde::{Deserialize, Serialize};
use time::{format_description::well_known::Rfc3339, Duration as TimeDuration, OffsetDateTime};

use super::{
    check_document_write_failure_point, collect_scan, decode_projected_row, encode_row,
    key_encoding, CassieError, DocumentRef, DocumentWriteFailurePoint, Midge, RowDecode, Uuid,
};
use crate::catalog::{IndexKind, IndexMeta};

#[derive(Debug, Clone)]
pub(crate) struct TimeSeriesIndexScanHit {
    pub id: String,
    pub bucket_key: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TimeSeriesIndexScanReport {
    pub hits: Vec<TimeSeriesIndexScanHit>,
    pub entries_scanned: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct TimeSeriesIndexRecord {
    collection: String,
    index_name: String,
    id: String,
    bucket_key: String,
    timestamp: String,
    generation: u64,
}

type TimeSeriesIndexEntry = (Vec<u8>, Vec<u8>);

impl Midge {
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
        if self.collection_uses_column_store(collection)? {
            return Err(CassieError::Unsupported(
                "fresh time-series document load requires row storage".to_string(),
            ));
        }
        if self
            .list_vector_indexes_canonical()?
            .iter()
            .any(|index| index.collection.eq_ignore_ascii_case(collection))
        {
            return Err(CassieError::Unsupported(
                "fresh time-series document load does not maintain vector indexes".to_string(),
            ));
        }

        let indexes = self
            .list_indexes()?
            .into_iter()
            .filter(|index| index.collection.eq_ignore_ascii_case(collection))
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
            .collection_schema(collection)
            .ok_or_else(|| CassieError::CollectionNotFound(collection.to_string()))?;
        let row_schema = self.row_schema(collection)?;
        let generation = self.collection_generation(collection)?.saturating_add(1);
        let write_gate = self.collection_write_gate(collection);
        let _write_guard = write_gate.lock();
        let mut tx = self.begin_data_rw_tx_for(collection)?;
        let mut ids = Vec::with_capacity(documents.len());

        for (id, payload) in documents {
            Self::validate_document(&schema, &payload)?;
            let id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
            let row_blob = encode_row(&row_schema, &payload)?;
            tx.put(Self::row_key(collection, &id), row_blob, None)
                .map_err(CassieError::from)?;
            Self::write_document_hash_to_tx(&mut tx, collection, &id, &row_schema, &payload)?;
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
        let generation = Self::increment_collection_generation_in_tx(&mut tx, collection)?;
        Self::record_column_batch_maintenance_debt_in_tx(&mut tx, collection, generation)?;
        Self::record_projection_hash_maintenance_debt_in_tx(&mut tx, collection, generation)?;
        Self::increment_data_epoch_in_tx(&mut tx)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        let _ = self.complete_column_batch_maintenance(collection, generation);
        let _ = self.complete_projection_hash_maintenance(collection, generation, row_delta);
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
                Some(payload) => Self::time_series_index_entry(index, id, payload, generation)?,
                None => None,
            };
            let new_entry = match new_payload {
                Some(payload) => Self::time_series_index_entry(index, id, payload, generation)?,
                None => None,
            };

            match (old_entry.as_ref(), new_entry.as_ref()) {
                (Some((old_key, old_value)), Some((new_key, new_value))) if old_key == new_key => {
                    if old_value != new_value {
                        tx.put(new_key.clone(), new_value.clone(), None)
                            .map_err(CassieError::from)?;
                        puts += 1;
                    }
                }
                _ => {
                    if let Some((old_key, _)) = old_entry {
                        tx.delete(old_key).map_err(CassieError::from)?;
                        deletes += 1;
                    }
                    if let Some((new_key, new_value)) = new_entry {
                        tx.put(new_key, new_value, None)
                            .map_err(CassieError::from)?;
                        puts += 1;
                    }
                }
            }
        }

        check_document_write_failure_point(DocumentWriteFailurePoint::TimeSeriesIndex)?;

        Ok((deletes, puts))
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

        let rows = self.scan_rows_for_rebuild(&index.collection, RowDecode::Full)?;
        let generation = self.collection_generation(&index.collection)?;
        let mut tx = self.begin_data_rw_tx_for(&index.collection)?;
        Self::delete_keys_with_prefix(
            &mut tx,
            Self::time_series_index_data_prefix(&index.collection, &index.name),
        )?;

        for row in rows {
            if let Some((key, value)) =
                Self::time_series_index_entry(index, &row.id, &row.payload, generation)?
            {
                tx.put(key, value, None).map_err(CassieError::from)?;
            }
        }

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    pub(crate) fn delete_time_series_index_data(
        &self,
        collection: &str,
        index_name: &str,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_data_rw_tx_for(collection)?;
        Self::delete_keys_with_prefix(
            &mut tx,
            Self::time_series_index_data_prefix(collection, index_name),
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
    ) -> Result<TimeSeriesIndexScanReport, CassieError> {
        if !Self::time_series_index_supports_storage(index) {
            return Err(CassieError::Unsupported(format!(
                "time-series index '{}' has unsupported bucket storage options",
                index.name
            )));
        }

        let tx = self.begin_data_readonly_tx_for(&index.collection)?;
        let query = match partition_key {
            Some(partition_key) => {
                let prefix = key_encoding::time_series_index_partition_prefix(
                    &index.collection,
                    &index.name,
                    partition_key,
                );
                let mut query = Query::new().prefix(prefix.clone().into());
                if let Some(lower) = lower_bucket_seconds {
                    query = query.start_key(
                        key_encoding::time_series_index_bucket_bound_key(
                            &index.collection,
                            &index.name,
                            partition_key,
                            lower,
                        )
                        .into(),
                    );
                }
                if let Some(upper) = upper_bucket_seconds {
                    query = query.end_key(
                        key_encoding::time_series_index_bucket_bound_key(
                            &index.collection,
                            &index.name,
                            partition_key,
                            upper,
                        )
                        .into(),
                    );
                }
                query
            }
            None => Query::new()
                .prefix(Self::time_series_index_data_prefix(&index.collection, &index.name).into()),
        };
        let scan = collect_scan(tx.scan(&query).map_err(CassieError::from)?)?;
        let entries_scanned = scan.len();
        let mut seen_ids = std::collections::BTreeSet::new();
        let mut hits = Vec::new();
        let generation = self.collection_generation(&index.collection)?;

        for (_key, raw_value) in scan {
            let record: TimeSeriesIndexRecord =
                serde_json::from_slice(&raw_value).map_err(|error| {
                    CassieError::Parse(format!(
                        "invalid time-series index entry for '{}': {error}",
                        index.name
                    ))
                })?;
            if record.collection != index.collection || record.index_name != index.name {
                return Err(CassieError::Parse(format!(
                    "invalid time-series index entry for '{}': mismatched metadata",
                    index.name
                )));
            }
            if record.generation != generation {
                return Err(CassieError::Unsupported(format!(
                    "time-series index '{}' has stale generation {} (current {})",
                    index.name, record.generation, generation
                )));
            }
            if seen_ids.insert(record.id.clone()) {
                hits.push(TimeSeriesIndexScanHit {
                    id: record.id,
                    bucket_key: record.bucket_key,
                    timestamp: record.timestamp,
                });
            }
        }

        Ok(TimeSeriesIndexScanReport {
            hits,
            entries_scanned,
        })
    }

    pub(crate) fn scan_time_series_hit_documents(
        &self,
        collection: &str,
        hits: &[TimeSeriesIndexScanHit],
        fields: &[String],
    ) -> Result<Vec<DocumentRef>, CassieError> {
        if hits.is_empty() {
            return Ok(Vec::new());
        }
        let row_schema = self.row_schema(collection)?;
        let projection = fields
            .iter()
            .map(|field| field.to_ascii_lowercase())
            .collect::<std::collections::HashSet<_>>();
        let tx = self.begin_data_readonly_tx_for(collection)?;
        let mut documents = Vec::with_capacity(hits.len());
        for hit in hits {
            let raw = match tx
                .get(&Self::row_key(collection, &hit.id))
                .map_err(CassieError::from)?
            {
                Some(raw) => Some(raw),
                None => tx
                    .get(&Self::doc_key(collection, &hit.id))
                    .map_err(CassieError::from)?,
            };
            let Some(raw) = raw else {
                continue;
            };
            let payload = decode_projected_row(&row_schema, &raw, &projection)?;
            documents.push(DocumentRef {
                id: hit.id.clone(),
                payload,
            });
        }

        Ok(documents)
    }

    fn time_series_index_supports_storage(index: &IndexMeta) -> bool {
        index.kind == IndexKind::TimeSeries
            && index.normalized_fields().len() == 1
            && bucket_width_duration(index).is_some()
    }

    fn time_series_index_entry(
        index: &IndexMeta,
        id: &str,
        payload: &serde_json::Value,
        generation: u64,
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
        let key = key_encoding::time_series_index_entry_key(
            &index.collection,
            &index.name,
            partition_key,
            bucket_start_seconds,
            id,
        );
        let value = serde_json::to_vec(&TimeSeriesIndexRecord {
            collection: index.collection.clone(),
            index_name: index.name.clone(),
            id: id.to_string(),
            bucket_key,
            timestamp: timestamp.to_string(),
            generation,
        })
        .map_err(|error| CassieError::Parse(error.to_string()))?;
        Ok(Some((key, value)))
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
