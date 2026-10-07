use std::mem::size_of;

use super::codec::normalized_vector_decoded_values_bytes;
use super::{decode_normalized_vector, CassieError, Midge, NormalizedVectorRecord, Query};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

impl Midge {
    pub(crate) fn list_normalized_vectors_controlled(
        &self,
        collection: &str,
        field: &str,
        controls: &QueryExecutionControls,
    ) -> Result<(Vec<NormalizedVectorRecord>, QueryMemoryReservation), CassieError> {
        check_controls(controls)?;
        let requested_collection = collection;
        let collection = self.canonical_collection_name(collection);
        let (relation_id, field_id) = self.vector_storage_ids(&collection, field)?;
        let prefix = Self::normalized_vector_prefix(relation_id, field_id);
        let _prefix_memory = controls.reserve_query_memory(prefix.len())?;
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let scan = tx
            .scan(&Query::new().prefix(prefix.clone().into()))
            .map_err(CassieError::from)?;
        let mut records = Vec::new();
        let mut memory = controls.reserve_query_memory(0)?;
        let generation = self.collection_generation(&collection)?;
        for entry in scan {
            check_controls(controls)?;
            let (key, raw) = entry.map_err(CassieError::from)?;
            self.record_query_scan_entry();
            if super::super::query_scan_control::should_cancel_controlled_query_scan(controls) {
                return Err(CassieError::QueryCancelled);
            }
            let Some(decoded_bytes) = normalized_vector_decoded_values_bytes(&raw) else {
                continue;
            };
            let previous = memory.bytes();
            let bytes = size_of::<NormalizedVectorRecord>()
                .checked_add(requested_collection.len())
                .and_then(|bytes| bytes.checked_add(field.len()))
                .and_then(|bytes| bytes.checked_add(key.len()))
                .and_then(|bytes| bytes.checked_add(raw.len()))
                .and_then(|bytes| bytes.checked_add(decoded_bytes))
                .ok_or_else(|| {
                    CassieError::ResourceLimit(
                        "normalized vector retained memory overflow".to_owned(),
                    )
                })?;
            memory.try_grow(bytes)?;
            let Some(id) = super::super::key_encoding::utf8_suffix_after_prefix(&key, &prefix)
            else {
                memory.shrink_to(previous);
                continue;
            };
            let record = match decode_normalized_vector(&raw, requested_collection, field, &id) {
                Ok(record) => record,
                Err(error @ CassieError::ResourceLimit(_)) => return Err(error),
                Err(_) => {
                    memory.shrink_to(previous);
                    continue;
                }
            };
            if record.built_generation != generation {
                return Ok((Vec::new(), controls.reserve_query_memory(0)?));
            }
            records.try_reserve_exact(1).map_err(|error| {
                CassieError::ResourceLimit(format!("unable to retain normalized vector: {error}"))
            })?;
            records.push(record);
        }
        check_controls(controls)?;
        if self.collection_generation(&collection)? != generation {
            return Ok((Vec::new(), controls.reserve_query_memory(0)?));
        }
        records.sort_unstable_by(|left, right| left.id.cmp(&right.id));
        Ok((records, memory))
    }

    pub(crate) fn get_normalized_vector_controlled(
        &self,
        collection: &str,
        field: &str,
        id: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Option<(NormalizedVectorRecord, QueryMemoryReservation)>, CassieError> {
        check_controls(controls)?;
        let requested_collection = collection;
        let collection = self.canonical_collection_name(collection);
        let (relation_id, field_id) = self.vector_storage_ids(&collection, field)?;
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let Some(raw) = tx
            .get(&Self::normalized_vector_key(relation_id, field_id, id))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        let Some(decoded_bytes) = normalized_vector_decoded_values_bytes(&raw) else {
            return Ok(None);
        };
        let bytes = size_of::<NormalizedVectorRecord>()
            .checked_add(raw.len())
            .and_then(|bytes| bytes.checked_add(decoded_bytes))
            .and_then(|bytes| bytes.checked_add(requested_collection.len()))
            .and_then(|bytes| bytes.checked_add(field.len()))
            .and_then(|bytes| bytes.checked_add(id.len()))
            .ok_or_else(|| {
                CassieError::ResourceLimit("normalized vector retained memory overflow".to_owned())
            })?;
        let memory = controls.reserve_query_memory(bytes)?;
        let record = decode_normalized_vector(&raw, requested_collection, field, id)?;
        if record.built_generation != self.collection_generation(&collection)? {
            return Ok(None);
        }
        check_controls(controls)?;
        Ok(Some((record, memory)))
    }
}

fn check_controls(controls: &QueryExecutionControls) -> Result<(), CassieError> {
    if controls.is_cancelled() {
        return Err(CassieError::QueryCancelled);
    }
    if controls.is_timed_out() {
        return Err(CassieError::DeadlineExceeded);
    }
    Ok(())
}
