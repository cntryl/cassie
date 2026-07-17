use cntryl_midge::Query;

use crate::catalog::IndexMeta;
use crate::runtime::accounted::AccountedVec;
use crate::runtime::QueryExecutionControls;

use super::codec::{covering_fields_retained_bytes, decode_covering_fields};
use super::{CassieError, Midge, ScalarIndexScanHit, ScalarIndexScanRequest};
use crate::midge::adapter::key_encoding;

impl Midge {
    pub(crate) fn scan_scalar_index(
        &self,
        index: &IndexMeta,
        request: &ScalarIndexScanRequest,
    ) -> Result<Vec<ScalarIndexScanHit>, CassieError> {
        let index = self.resolve_scalar_scan_index(index)?;
        if request.limit == Some(0) {
            return Ok(Vec::new());
        }
        let query = Self::scalar_index_query(&index, request)?;
        let tx = self.begin_data_readonly_tx_for(&index.collection)?;
        let scan = tx.scan(&query).map_err(CassieError::from)?;
        let mut hits = Vec::new();
        for entry in scan {
            let (key, raw_value) = entry.map_err(CassieError::from)?;
            hits.push(decode_scalar_index_hit(&index, &key, &raw_value)?);
        }
        Ok(hits)
    }

    pub(crate) fn scan_scalar_index_controlled(
        &self,
        index: &IndexMeta,
        request: &ScalarIndexScanRequest,
        controls: &QueryExecutionControls,
    ) -> Result<AccountedVec<ScalarIndexScanHit>, CassieError> {
        check_controls(controls)?;
        let index = self.resolve_scalar_scan_index(index)?;
        let mut hits = AccountedVec::try_new(controls)?;
        if request.limit == Some(0) {
            return Ok(hits);
        }

        let query = Self::scalar_index_query(&index, request)?;
        let tx = self.begin_data_readonly_tx_for(&index.collection)?;
        let mut scan = tx.scan(&query).map_err(CassieError::from)?;
        loop {
            check_controls(controls)?;
            let Some(entry) = scan.next() else {
                break;
            };
            check_controls(controls)?;
            let (key, raw_value) = entry.map_err(CassieError::from)?;
            self.record_query_scan_entry();
            if super::super::query_scan_control::should_cancel_controlled_query_scan() {
                return Err(CassieError::QueryCancelled);
            }
            let retained_bytes = scalar_index_hit_variable_bytes(&key, &raw_value)?;
            hits.try_push_with_result(retained_bytes, || {
                decode_scalar_index_hit(&index, &key, &raw_value)
            })?;
        }
        check_controls(controls)?;
        Ok(hits)
    }

    fn resolve_scalar_scan_index(&self, index: &IndexMeta) -> Result<IndexMeta, CassieError> {
        let mut index = index.clone();
        index.collection = self.canonical_collection_name(&index.collection);
        if index.storage_id().is_none() || index.relation_id().is_none() {
            let stored = self
                .get_index(&index.collection, &index.name)?
                .ok_or_else(|| CassieError::Parse(format!("index '{}' not found", index.name)))?;
            index.set_storage_ids(
                stored.relation_id().ok_or_else(|| {
                    CassieError::Parse(format!("index '{}' is missing its relation id", index.name))
                })?,
                stored.storage_id().ok_or_else(|| {
                    CassieError::Parse(format!("index '{}' is missing its storage id", index.name))
                })?,
            );
        }
        if !Self::scalar_index_supports_storage(&index) {
            return Err(CassieError::Unsupported(format!(
                "scalar index '{}' is not storage-backed",
                index.name
            )));
        }
        Ok(index)
    }

    fn scalar_index_query(
        index: &IndexMeta,
        request: &ScalarIndexScanRequest,
    ) -> Result<Query, CassieError> {
        let (relation_id, index_id) = Self::scalar_index_storage_ids(index)?;
        let data_prefix = Self::scalar_index_data_prefix(relation_id, index_id);
        let seek_prefix = key_encoding::scalar_index_seek_prefix(
            relation_id,
            index_id,
            &request.equality_prefix,
        )?;
        let (start_key, end_key) = key_encoding::scalar_index_query_bounds(
            &seek_prefix,
            request.lower_bound.as_ref(),
            request.upper_bound.as_ref(),
        )?;
        let mut query = Query::new().prefix(data_prefix.into());
        if let Some(start_key) = start_key {
            query = query.start_key(start_key.into());
        }
        if let Some(end_key) = end_key {
            query = query.end_key(end_key.into());
        }
        if request.reverse {
            query = query.reverse();
        }
        if let Some(limit) = request.limit {
            query = query.limit(limit);
        }
        Ok(query)
    }
}

fn decode_scalar_index_hit(
    index: &IndexMeta,
    key: &[u8],
    raw_value: &[u8],
) -> Result<ScalarIndexScanHit, CassieError> {
    let id = key_encoding::utf8_last_component(key).ok_or_else(|| {
        CassieError::Parse(format!("invalid scalar index key for '{}'", index.name))
    })?;
    let fields = if raw_value.is_empty() {
        serde_json::Map::new()
    } else {
        decode_covering_fields(raw_value)?.into_iter().collect()
    };
    Ok(ScalarIndexScanHit { id, fields })
}

fn scalar_index_hit_variable_bytes(key: &[u8], raw_value: &[u8]) -> Result<usize, CassieError> {
    let fields = if raw_value.is_empty() {
        0
    } else {
        covering_fields_retained_bytes(raw_value)?
    };
    key.len().checked_add(fields).ok_or_else(|| {
        CassieError::ResourceLimit("scalar index hit accounting overflow".to_owned())
    })
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
