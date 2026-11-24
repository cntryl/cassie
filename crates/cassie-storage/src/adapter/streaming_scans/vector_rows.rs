//! Heap-table row pages that decode one vector field straight into `f32` values.
//!
//! Exact vector search ranks rows by their vector alone. This reader pages row blobs the way the
//! generic cursor does, but never turns the vector into JSON: each row is charged for the `f32`
//! buffer it actually holds. Column-store tables keep the generic reader.

use std::mem::size_of;

use super::{
    check_controls, key_encoding, CassieError, CursorStorage, Midge, MidgeRowCursor, Query,
    STORAGE_SCAN_PAGE_ENTRIES,
};
use crate::midge::row_blob::{split_vector_row, PackedVector, VectorColumn};
use crate::runtime::accounted::json;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

/// One row read for vector ranking, with the reservation that covers what it retains.
pub(crate) struct AccountedVectorRow {
    pub(crate) id: String,
    /// `None` when the row has no vector value.
    pub(crate) vector: Option<Vec<f32>>,
    /// The other fields, present only when the caller asked for them.
    pub(crate) payload: Option<serde_json::Value>,
    _memory: QueryMemoryReservation,
}

impl MidgeRowCursor {
    /// The vector field to decode directly, or `None` when this cursor reads column storage or
    /// the field is not a stored `VECTOR` column, so the caller keeps the generic reader.
    pub(crate) fn vector_column(&self, field: &str) -> Option<VectorColumn> {
        (self.storage == CursorStorage::Row && self.pending.is_none())
            .then(|| VectorColumn::resolve(&self.row_schema, field))
            .flatten()
    }

    /// Reads up to `limit` rows, decoding `column` directly and, with `with_payload`, the other
    /// fields as the generic decoder would.
    pub(crate) fn next_vector_rows(
        &mut self,
        midge: &Midge,
        limit: usize,
        controls: &QueryExecutionControls,
        column: &VectorColumn,
        with_payload: bool,
    ) -> Result<Vec<AccountedVectorRow>, CassieError> {
        let mut rows = Vec::new();
        while !self.exhausted && rows.len() < limit {
            self.read_vector_page(midge, limit, controls, column, with_payload, &mut rows)?;
        }
        Ok(rows)
    }

    fn read_vector_page(
        &mut self,
        midge: &Midge,
        result_limit: usize,
        controls: &QueryExecutionControls,
        column: &VectorColumn,
        with_payload: bool,
        rows: &mut Vec<AccountedVectorRow>,
    ) -> Result<(), CassieError> {
        check_controls(controls)?;
        let storage_limit = result_limit
            .saturating_sub(rows.len())
            .clamp(1, STORAGE_SCAN_PAGE_ENTRIES);
        let query_key_bytes = self.prefix.len().saturating_add(
            self.last_key
                .as_ref()
                .map_or(0, |key| key.len().saturating_add(1)),
        );
        let _query_key_memory = controls.reserve_query_memory(query_key_bytes)?;
        let mut query = Query::new()
            .prefix(self.prefix.clone().into())
            .limit(storage_limit);
        if let Some(last_key) = self.last_key.as_ref() {
            let mut next_key = last_key.clone();
            next_key.push(0);
            query = query.start_key(next_key.into());
        }
        let mut scan = self.tx.scan(&query).map_err(CassieError::from)?;
        let mut scanned_entries = 0usize;
        while rows.len() < result_limit {
            check_controls(controls)?;
            let Some(entry) = scan.next() else {
                if scanned_entries < storage_limit {
                    self.exhausted = true;
                }
                break;
            };
            let (raw_key, raw_value) = entry.map_err(CassieError::from)?;
            scanned_entries = scanned_entries.saturating_add(1);
            midge.record_query_scan_entry();
            if super::super::query_scan_control::should_cancel_controlled_query_scan(controls) {
                return Err(CassieError::QueryCanceled);
            }
            let next_key_memory = controls.reserve_query_memory(raw_key.len())?;
            self.last_key = Some(raw_key.to_vec());
            self.last_key_memory = Some(next_key_memory);
            let Some(id) = key_encoding::utf8_suffix_after_prefix(&raw_key, &self.prefix) else {
                continue;
            };
            if id.is_empty() {
                continue;
            }
            let row = build_vector_row(
                controls,
                &self.row_schema,
                column,
                id,
                &raw_value,
                with_payload,
            )?;
            rows.try_reserve_exact(1).map_err(|error| {
                CassieError::ResourceLimit(format!(
                    "unable to retain controlled vector page: {error}"
                ))
            })?;
            rows.push(row);
        }
        Ok(())
    }
}

fn build_vector_row(
    controls: &QueryExecutionControls,
    schema: &crate::midge::row_blob::RowSchema,
    column: &VectorColumn,
    id: String,
    raw_value: &[u8],
    with_payload: bool,
) -> Result<AccountedVectorRow, CassieError> {
    let parts = split_vector_row(schema, column, raw_value, with_payload)?;
    let vector_bytes = parts.vector.as_ref().map_or(0, PackedVector::decoded_bytes);
    let payload_bytes = parts
        .payload
        .as_ref()
        .map_or(Ok(0), json::retained_decoded_bytes)?;
    let retained = size_of::<AccountedVectorRow>()
        .saturating_add(id.len())
        .saturating_add(vector_bytes)
        .saturating_add(payload_bytes);
    let memory = controls.reserve_query_memory(retained)?;
    let vector = parts.vector.map(PackedVector::to_f32).transpose()?;
    Ok(AccountedVectorRow {
        id,
        vector,
        payload: parts.payload,
        _memory: memory,
    })
}
