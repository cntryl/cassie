use cntryl_midge::{Query, WriteOptions};
use serde::{Deserialize, Serialize};
use time::{format_description::well_known::Rfc3339, Duration as TimeDuration, OffsetDateTime};

use super::{
    check_document_write_failure_point, decode_projected_row, encode_row, key_encoding,
    CassieError, DocumentRef, DocumentWriteFailurePoint, Midge, RowDecode, Uuid,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct TimeSeriesIndexRecord {
    collection: String,
    index_name: String,
    id: String,
    bucket_key: String,
    timestamp: String,
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
            .list_vector_indexes()?
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
        let write_gate = self.collection_write_gate(collection);
        let _write_guard = write_gate.lock();
        let mut tx = self.begin_data_rw_tx()?;
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
            )?;
            ids.push(id);
        }

        let row_delta = i64::try_from(ids.len()).unwrap_or(i64::MAX);
        Self::increment_data_epoch_in_tx(&mut tx)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        self.refresh_projection_hashes_after_write(collection, row_delta)?;
        Ok(ids)
    }

    pub(crate) fn sync_time_series_indexes_for_document(
        tx: &mut cntryl_midge::Transaction,
        id: &str,
        old_payload: Option<&serde_json::Value>,
        new_payload: Option<&serde_json::Value>,
        indexes: &[IndexMeta],
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
        let mut tx = self.begin_data_rw_tx()?;
        Self::delete_keys_with_prefix(
            &mut tx,
            Self::time_series_index_data_prefix(&index.collection, &index.name),
        )?;

        for row in rows {
            if let Some((key, value)) = Self::time_series_index_entry(index, &row.id, &row.payload)?
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
        let mut tx = self.begin_data_rw_tx()?;
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
    ) -> Result<TimeSeriesIndexScanReport, CassieError> {
        if !Self::time_series_index_supports_storage(index) {
            return Err(CassieError::Unsupported(format!(
                "time-series index '{}' has unsupported bucket storage options",
                index.name
            )));
        }

        let tx = self.begin_data_readonly_tx()?;
        let scan =
            tx.scan(&Query::new().prefix(
                Self::time_series_index_data_prefix(&index.collection, &index.name).into(),
            ))
            .map_err(CassieError::from)?;
        let mut seen_ids = std::collections::BTreeSet::new();
        let mut hits = Vec::new();

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
            if seen_ids.insert(record.id.clone()) {
                hits.push(TimeSeriesIndexScanHit {
                    id: record.id,
                    bucket_key: record.bucket_key,
                    timestamp: record.timestamp,
                });
            }
        }

        Ok(TimeSeriesIndexScanReport { hits })
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
        let mut wanted = hits
            .iter()
            .map(|hit| hit.id.clone())
            .collect::<std::collections::HashSet<_>>();
        let tx = self.begin_data_readonly_tx()?;
        let mut documents = Vec::with_capacity(wanted.len());
        let mut seen = std::collections::HashSet::new();

        for (prefix, include_seen) in [
            (Self::row_prefix(collection), true),
            (Self::doc_prefix(collection), false),
        ] {
            let iter = tx
                .scan(&Query::new().prefix(prefix.clone().into()))
                .map_err(CassieError::from)?;
            for (raw_key, raw_value) in iter {
                let Some(id) = key_encoding::utf8_suffix_after_prefix(&raw_key, &prefix) else {
                    continue;
                };
                if id.is_empty() || !wanted.contains(&id) || (!include_seen && seen.contains(&id)) {
                    continue;
                }
                let payload = decode_projected_row(&row_schema, &raw_value, &projection)?;
                seen.insert(id.clone());
                wanted.remove(&id);
                documents.push(DocumentRef { id, payload });
                if wanted.is_empty() {
                    return Ok(documents);
                }
            }
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
        let key = key_encoding::time_series_index_entry_key(
            &index.collection,
            &index.name,
            &bucket_key,
            id,
        );
        let value = serde_json::to_vec(&TimeSeriesIndexRecord {
            collection: index.collection.clone(),
            index_name: index.name.clone(),
            id: id.to_string(),
            bucket_key,
            timestamp: timestamp.to_string(),
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
