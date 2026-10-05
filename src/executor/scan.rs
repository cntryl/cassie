use crate::app::{Cassie, CassieError, CassieSession, SessionRowCursor};
use crate::catalog::{CollectionSchema, IndexKind};
use crate::executor::batch::{Batch, BatchRow, BatchStream, DEFAULT_BATCH_SIZE};
use crate::executor::QueryError;
use crate::midge::adapter::RowFilter;
use crate::midge::adapter::{
    ColumnBatchScanDecision, ColumnBatchScanFilter, ControlledColumnBatchScanRequest, DocumentRef,
    RowDecode,
};
use crate::runtime::{
    column_batch_metrics::ColumnBatchScanMetrics, QueryExecutionControls, QueryMemoryReservation,
};
use crate::types::row_identity::{
    is_identity_reference, is_row_identity_column, ROW_IDENTITY_COLUMN,
};
use crate::types::{DataType, Value, Vector};
use std::collections::HashSet;
use std::time::Duration;

mod controlled_source;
mod conversion;
mod ordered_projection_accounting;

pub(crate) use ordered_projection_accounting::ordered_projection_shape;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ScanTimings {
    pub(crate) scan: Duration,
    pub(crate) row_decode: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProjectedDocumentFilter {
    pub(crate) field: String,
    pub(crate) value: Value,
}

pub(crate) struct ProjectedScanStream<'a> {
    cassie: &'a Cassie,
    cursor: SessionRowCursor,
    fields: Vec<String>,
    schema: Option<CollectionSchema>,
    controls: &'a QueryExecutionControls,
    remaining: usize,
    previous_page_memory: Option<QueryMemoryReservation>,
}

impl BatchStream for ProjectedScanStream<'_> {
    fn next_batch(&mut self) -> Result<Option<Batch>, crate::executor::QueryError> {
        self.previous_page_memory = None;
        if self.remaining == 0 {
            return Ok(None);
        }
        let batch_size = self.remaining.min(DEFAULT_BATCH_SIZE);
        let accounted_documents = self
            .cursor
            .next_accounted_documents(&self.cassie.midge, batch_size, self.controls)
            .map_err(|error| controlled_storage_error(self.cassie, error))?;
        if accounted_documents.is_empty() {
            return Ok(None);
        }
        self.remaining = self.remaining.saturating_sub(accounted_documents.len());
        let _input_memory = self
            .controls
            .reserve_query_memory(std::mem::size_of::<conversion::InputBatch>())?;
        let converted = conversion::convert(
            self.cassie,
            vec![conversion::InputBatch::Accounted(accounted_documents)],
            &conversion::Request {
                schema: self.schema.as_ref(),
                controls: self.controls,
                shape: conversion::Shape::Projected {
                    fields: &self.fields,
                    filter: None,
                },
                parallel: false,
                #[cfg(test)]
                after_row: None,
            },
        )?;
        self.previous_page_memory = Some(converted.memory);
        Ok(converted.batches.into_iter().next())
    }
}

pub(crate) fn projected_scan_stream<'a>(
    cassie: &'a Cassie,
    session: Option<&CassieSession>,
    collection: &str,
    fields: &[String],
    limit: Option<usize>,
    controls: &'a QueryExecutionControls,
) -> Result<Option<ProjectedScanStream<'a>>, crate::executor::QueryError> {
    if !uses_controlled_row_scan(cassie, collection) {
        return Ok(None);
    }
    let Some(cursor) = cassie
        .open_session_row_cursor(
            session,
            collection,
            RowDecode::ProjectedHistorical(fields.to_vec()),
            controls,
        )
        .map_err(|error| controlled_storage_error(cassie, error))?
    else {
        return Ok(None);
    };
    cassie.runtime.record_parallel_scan_fallback();
    Ok(Some(ProjectedScanStream {
        cassie,
        cursor,
        fields: fields.to_vec(),
        schema: cassie.catalog.get_schema(collection),
        controls,
        remaining: limit.unwrap_or(usize::MAX),
        previous_page_memory: None,
    }))
}

pub(crate) fn collect_projected_stream_rows(
    stream: &mut ProjectedScanStream<'_>,
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, crate::executor::QueryError> {
    let (rows, memory) = crate::executor::batch::collect_batch_stream_accounted(stream, controls)?;
    stream
        .cassie
        .runtime
        .record_storage_access("data", false, true);
    drop(memory);
    Ok(rows)
}

pub(crate) fn scan(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    collection: &str,
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, crate::executor::QueryError> {
    scan_limit(cassie, session, collection, None, controls)
}

pub(crate) fn scan_limit(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    collection: &str,
    limit: Option<usize>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, crate::executor::QueryError> {
    let (batches, _memory) = scan_limit_retained(cassie, session, collection, limit, controls)?;
    Ok(batches)
}

/// Keeps the existing full-conversion admission alive for a consuming bounded operator.
pub(crate) fn scan_limit_retained(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    collection: &str,
    limit: Option<usize>,
    controls: &QueryExecutionControls,
) -> Result<(Vec<Batch>, QueryMemoryReservation), crate::executor::QueryError> {
    if let Some(cursor) = cassie
        .open_session_row_cursor(session, collection, RowDecode::Full, controls)
        .map_err(|error| controlled_storage_error(cassie, error))?
    {
        let schema = cassie.catalog.get_schema(collection);
        let source = controlled_source::collect(cassie, cursor, limit, controls)?;
        let converted = conversion::convert(
            cassie,
            source.batches,
            &conversion::Request {
                schema: schema.as_ref(),
                controls,
                shape: conversion::Shape::Full,
                parallel: !uses_controlled_row_scan(cassie, collection),
                #[cfg(test)]
                after_row: None,
            },
        )?;
        conversion::check_controls(controls)?;
        cassie.runtime.record_storage_access("data", false, true);
        record_collection_scan(cassie, collection, schema.as_ref(), &converted.batches);
        drop(source.memory);
        return Ok((converted.batches, converted.memory));
    }

    let document_batches = cassie
        .scan_documents_batched_for_session_limit(session, collection, DEFAULT_BATCH_SIZE, limit)
        .map_err(|error| {
            cassie.runtime.record_storage_access("data", false, false);
            crate::executor::QueryError::General(error.to_string())
        })?;
    cassie.runtime.record_storage_access("data", false, true);
    let schema = cassie.catalog.get_schema(collection);

    let converted = conversion::convert_owned(
        cassie,
        document_batches,
        &conversion::Request {
            schema: schema.as_ref(),
            controls,
            shape: conversion::Shape::Full,
            parallel: true,
            #[cfg(test)]
            after_row: None,
        },
    )?;
    record_collection_scan(cassie, collection, schema.as_ref(), &converted.batches);

    Ok((converted.batches, converted.memory))
}

fn record_collection_scan(
    cassie: &Cassie,
    collection: &str,
    schema: Option<&CollectionSchema>,
    batches: &[Batch],
) {
    let rows = batches.iter().map(Vec::len).sum::<usize>();
    let fields = schema.map_or(0, |schema| schema.fields.len());
    cassie
        .runtime
        .record_read_path_collection_scan(collection, fields, rows);
}

fn uses_controlled_row_scan(cassie: &Cassie, collection: &str) -> bool {
    cassie.runtime.limits().parallel_scan_workers.max(1) == 1
        || cassie
            .catalog
            .collection_storage_mode(collection)
            .is_some_and(
                crate::catalog::collections::CollectionStorageMode::uses_column_store_storage,
            )
}

pub(crate) struct ProjectedFilteredScanRequest<'a> {
    pub(crate) collection: &'a str,
    pub(crate) fields: &'a [String],
    pub(crate) limit: Option<usize>,
    pub(crate) document_filter: Option<&'a ProjectedDocumentFilter>,
    pub(crate) column_filter: Option<&'a ColumnBatchScanFilter>,
    pub(crate) controls: &'a QueryExecutionControls,
}

pub(crate) struct EncodedProjectedScan {
    pub(crate) batches: Vec<Batch>,
    pub(crate) timings: ScanTimings,
}

pub(crate) fn scan_projected_filtered(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    request: &ProjectedFilteredScanRequest<'_>,
) -> Result<Vec<Batch>, crate::executor::QueryError> {
    scan_projected_filtered_with_timings(cassie, session, request).map(|(batches, _)| batches)
}

pub(crate) fn scan_projected_filtered_with_timings(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    request: &ProjectedFilteredScanRequest<'_>,
) -> Result<(Vec<Batch>, ScanTimings), crate::executor::QueryError> {
    let storage_filter = request
        .document_filter
        .and_then(row_filter_from_projected_filter);
    let has_session_changes =
        session.is_some_and(|session| session.has_collection_changes(request.collection));
    if !has_session_changes {
        if let Some(result) =
            try_controlled_column_batch_scan(cassie, request, storage_filter.as_ref())?
        {
            return Ok((result.batches, result.timings));
        }
    }
    if let Some(fallback) = controlled_projected_row_fallback(cassie, session, request)? {
        let schema = cassie.catalog.get_schema(request.collection);
        let converted = conversion::convert(
            cassie,
            fallback.batches,
            &conversion::Request {
                schema: schema.as_ref(),
                controls: request.controls,
                shape: conversion::Shape::Projected {
                    fields: request.fields,
                    filter: request.document_filter,
                },
                parallel: !uses_controlled_row_scan(cassie, request.collection),
                #[cfg(test)]
                after_row: None,
            },
        )?;
        let batches = converted.batches;
        if has_session_changes
            && has_covering_column_index(cassie, request.collection, request.fields)
        {
            let rows = batches.iter().map(Vec::len).sum::<usize>();
            cassie
                .runtime
                .record_column_batch_row_blob_fallback(rows, "session-changes");
        }
        cassie.runtime.record_storage_access("data", false, true);
        drop(fallback.memory);
        return Ok((batches, ScanTimings::default()));
    }
    let (document_batches, raw_timings) = cassie
        .scan_projected_documents_batched_for_session_with_filter_and_timings(
            session,
            request.collection,
            DEFAULT_BATCH_SIZE,
            request.fields,
            storage_filter.as_ref(),
            request.limit,
        )
        .map_err(|error| {
            cassie.runtime.record_storage_access("data", false, false);
            crate::executor::QueryError::General(error.to_string())
        })?;
    cassie.runtime.record_storage_access("data", false, true);

    let mut timings = ScanTimings {
        scan: raw_timings.scan,
        row_decode: raw_timings.row_decode,
    };
    let materialize_started = std::time::Instant::now();
    let schema = cassie.catalog.get_schema(request.collection);
    let converted = projected_document_batches_to_rows(
        cassie,
        document_batches,
        request.fields,
        request.document_filter,
        schema.as_ref(),
        request.controls,
    )?;
    let batches = converted.batches;
    if has_session_changes && has_covering_column_index(cassie, request.collection, request.fields)
    {
        let rows = batches.iter().map(Vec::len).sum::<usize>();
        cassie
            .runtime
            .record_column_batch_row_blob_fallback(rows, "session-changes");
    }
    timings.scan += materialize_started.elapsed();

    Ok((batches, timings))
}

pub(crate) fn try_controlled_column_batch_scan(
    cassie: &Cassie,
    request: &ProjectedFilteredScanRequest<'_>,
    storage_filter: Option<&RowFilter>,
) -> Result<Option<EncodedProjectedScan>, crate::executor::QueryError> {
    let outcome = cassie.midge.scan_column_batch_projected_rows_controlled(
        &ControlledColumnBatchScanRequest {
            collection: request.collection,
            batch_size: DEFAULT_BATCH_SIZE,
            fields: request.fields,
            filter: storage_filter,
            segment_filter: request.column_filter,
            limit: request.limit,
            controls: request.controls,
        },
    );
    match outcome {
        Ok(ColumnBatchScanDecision::Hit(outcome)) => {
            let query_memory = outcome.query_memory;
            cassie.runtime.record_storage_access("data", false, true);
            let schema = cassie.catalog.get_schema(request.collection);
            let mut timings = ScanTimings {
                scan: outcome.timings.scan,
                row_decode: outcome.timings.row_decode,
            };
            let materialize_started = std::time::Instant::now();
            let converted = projected_document_batches_to_rows(
                cassie,
                outcome.batches,
                request.fields,
                request.document_filter,
                schema.as_ref(),
                request.controls,
            )?;
            let batches = converted.batches;
            timings.scan += materialize_started.elapsed();
            let rows = batches.iter().map(Vec::len).sum::<usize>();
            cassie
                .runtime
                .record_column_batch_scan(&ColumnBatchScanMetrics {
                    rows,
                    segments_read: outcome.segments_read,
                    chunks_read: outcome.chunks_read,
                    physical_bytes: outcome.physical_bytes,
                    logical_bytes: outcome.logical_bytes,
                    predicate_values: outcome.predicate_values,
                    candidate_rows: outcome.candidate_rows,
                    selected_rows: outcome.selected_rows,
                    materialized_values: outcome.materialized_values,
                    skipped_segments: outcome.skipped_segments,
                });
            drop(query_memory);
            Ok(Some(EncodedProjectedScan { batches, timings }))
        }
        Ok(ColumnBatchScanDecision::Fallback(reason)) => {
            record_column_batch_fallback(cassie, request, reason);
            Ok(None)
        }
        Err(
            error @ (CassieError::QueryCancelled
            | CassieError::DeadlineExceeded
            | CassieError::ResourceLimit(_)),
        ) => Err(crate::executor::QueryError::from(error)),
        Err(error) => {
            cassie.runtime.record_column_batch_fallback("error");
            cassie.runtime.record_storage_access("data", false, false);
            Err(crate::executor::QueryError::General(error.to_string()))
        }
    }
}

fn record_column_batch_fallback(
    cassie: &Cassie,
    request: &ProjectedFilteredScanRequest<'_>,
    reason: crate::midge::adapter::ColumnBatchScanFallbackReason,
) {
    if !has_covering_column_index(cassie, request.collection, request.fields) {
        return;
    }
    if reason.is_decode_fallback() {
        cassie
            .runtime
            .record_column_batch_decode_fallback_with_reason(reason.as_str());
    } else {
        cassie.runtime.record_column_batch_fallback(reason.as_str());
    }
}

fn controlled_projected_row_fallback(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    request: &ProjectedFilteredScanRequest<'_>,
) -> Result<Option<controlled_source::ControlledInputs>, crate::executor::QueryError> {
    let Some(cursor) = cassie
        .open_session_row_cursor(
            session,
            request.collection,
            RowDecode::ProjectedHistorical(request.fields.to_vec()),
            request.controls,
        )
        .map_err(|error| controlled_storage_error(cassie, error))?
    else {
        return Ok(None);
    };
    controlled_source::collect(cassie, cursor, request.limit, request.controls).map(Some)
}

fn controlled_storage_error(cassie: &Cassie, error: CassieError) -> crate::executor::QueryError {
    if !matches!(
        error,
        CassieError::QueryCancelled | CassieError::DeadlineExceeded | CassieError::ResourceLimit(_)
    ) {
        cassie.runtime.record_storage_access("data", false, false);
    }
    crate::executor::QueryError::from(error)
}

pub(crate) fn record_streamed_column_batch_fallback(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    projection: (&str, &[String]),
    rows: usize,
) {
    let (collection, fields) = projection;
    if session.is_some_and(|session| session.has_collection_changes(collection))
        && has_covering_column_index(cassie, collection, fields)
    {
        cassie
            .runtime
            .record_column_batch_row_blob_fallback(rows, "session-changes");
    }
}

fn has_covering_column_index(cassie: &Cassie, collection: &str, fields: &[String]) -> bool {
    let wanted = fields
        .iter()
        .filter(|field| !is_row_identity_column(field))
        .filter_map(|field| crate::sql::ColumnIdentifierPath::parse(field).ok())
        .map(|field| field.field_lookup_key())
        .collect::<HashSet<_>>();
    !wanted.is_empty()
        && cassie
            .catalog
            .list_indexes(collection)
            .into_iter()
            .any(|index| {
                index.kind == IndexKind::Column
                    && wanted.iter().all(|field| {
                        index.normalized_fields().iter().any(|candidate| {
                            crate::sql::ColumnIdentifierPath::from_field_name(candidate)
                                .lookup_key()
                                == *field
                        })
                    })
            })
}

fn row_filter_from_projected_filter(filter: &ProjectedDocumentFilter) -> Option<RowFilter> {
    Some(RowFilter {
        field: filter.field.clone(),
        value: value_to_json(&filter.value)?,
    })
}

fn value_to_json(value: &Value) -> Option<serde_json::Value> {
    match value {
        Value::Null => Some(serde_json::Value::Null),
        Value::Bool(value) => Some(serde_json::Value::Bool(*value)),
        Value::Int64(value) => Some(serde_json::Value::Number((*value).into())),
        Value::Float64(value) => {
            serde_json::Number::from_f64(*value).map(serde_json::Value::Number)
        }
        Value::String(value) => Some(serde_json::Value::String(value.clone())),
        Value::Vector(_) | Value::Json(_) => None,
    }
}

fn document_to_row(document: &DocumentRef, schema: Option<&CollectionSchema>) -> BatchRow {
    let schema_has_id = schema.is_some_and(CollectionSchema::declares_id);
    let mut row = Vec::new();
    push_row_identity(&mut row, &document.id);
    if let Some(obj) = document.payload.as_object() {
        if let Some(schema) = schema.as_ref() {
            let mut seen = HashSet::new();
            for field in &schema.fields {
                if is_row_identity_column(&field.name) {
                    continue;
                }
                let value = obj.get(&field.name).map_or(Value::Null, |value| {
                    json_to_typed_value(value, &field.data_type)
                });
                row.push((field.name.clone(), value));
                seen.insert(field.name.clone());
            }
            for (k, v) in obj {
                if seen.contains(k) || is_row_identity_column(k) {
                    continue;
                }
                // When the schema declares no `id` field, `id` names
                // the internal identity already pushed above, and
                // `aggregate::columns_from_projection` emits exactly
                // one column for it. SQL writes cannot store such a
                // key, but a payload from another write path could;
                // dropping it keeps the row from being one value
                // wider than its own column list.
                if is_identity_reference(k, schema_has_id) {
                    continue;
                }
                row.push((k.clone(), json_to_value(v)));
            }
        } else {
            for (k, v) in obj {
                if is_identity_reference(k, false) {
                    continue;
                }
                row.push((k.clone(), json_to_value(v)));
            }
        }
    }
    typed_row(BatchRow::new(row), schema)
}

/// Every document has an internal identity that Cassie must always be able
/// to resolve regardless of the user's schema (DML target resolution,
/// retention, and scored retrieval all depend on it), so it is always
/// pushed under the reserved `_id` key — and only that key.
///
/// A bare `id` never reaches row building on a table that declares no `id`
/// field: `planner::logical::rewrite_reserved_id_references` renames it to
/// `_id` at the logical-plan level first. A table that does declare `id`
/// gets that field pushed as an ordinary column by the caller's own field
/// loop. Neither case needs a second physical copy of the identity value in
/// every row, so nothing is stored under `id` here. `SELECT *` resolves its
/// leading `id` column from this entry at the final result boundary (see
/// `execution::result::build_select_result`).
pub(crate) fn push_row_identity(row: &mut Vec<(String, Value)>, document_id: &str) {
    // Only `_id` is stored physically. Every reference to a bare `id` on a
    // table without its own `id` field was already rewritten to `_id` at
    // the logical-plan level (see
    // `planner::logical::rewrite_reserved_id_references`) before reaching
    // any code that reads rows, so nothing needs a duplicate `id` entry
    // here — except the raw `SELECT *` value dump, which resolves it
    // separately at the final result boundary (see
    // `execution::result::build_select_result`) instead of paying for a
    // second copy of the value in every row.
    row.push((
        ROW_IDENTITY_COLUMN.to_string(),
        Value::String(document_id.to_string()),
    ));
}

fn projected_document_batches_to_rows(
    cassie: &Cassie,
    document_batches: Vec<Vec<DocumentRef>>,
    fields: &[String],
    document_filter: Option<&ProjectedDocumentFilter>,
    schema: Option<&CollectionSchema>,
    controls: &QueryExecutionControls,
) -> Result<conversion::ConvertedBatches, QueryError> {
    conversion::convert_owned(
        cassie,
        document_batches,
        &conversion::Request {
            schema,
            controls,
            shape: conversion::Shape::Projected {
                fields,
                filter: document_filter,
            },
            parallel: true,
            #[cfg(test)]
            after_row: None,
        },
    )
}

pub(crate) fn projected_document_to_row(
    document: &DocumentRef,
    fields: &[String],
    schema: Option<&CollectionSchema>,
) -> BatchRow {
    let schema_has_id = schema.is_some_and(CollectionSchema::declares_id);
    let mut row = Vec::with_capacity(fields.len() + 2);
    push_row_identity(&mut row, &document.id);
    let object = document.payload.as_object();
    for field in fields {
        if is_identity_reference(field, schema_has_id) {
            continue;
        }
        let value = object
            .and_then(|object| projected_field_value(object, field))
            .map_or(Value::Null, |value| {
                field_data_type(schema, field).map_or_else(
                    || json_to_value(value),
                    |data_type| json_to_typed_value(value, data_type),
                )
            });
        row.push((field.clone(), value));
    }
    typed_row(BatchRow::from_projected_values(row), schema)
}

fn typed_row(mut row: BatchRow, schema: Option<&CollectionSchema>) -> BatchRow {
    attach_row_types(&mut row, schema);
    row
}

pub(crate) fn attach_row_types(row: &mut BatchRow, schema: Option<&CollectionSchema>) {
    if !schema.is_some_and(|schema| {
        schema
            .fields
            .iter()
            .any(|field| matches!(field.data_type, DataType::Array(_)))
    }) {
        return;
    }
    let types = row
        .entries()
        .iter()
        .map(|(name, _)| {
            field_data_type(schema, name)
                .cloned()
                .unwrap_or(DataType::Null)
        })
        .collect::<Vec<_>>()
        .into();
    row.set_data_types(types);
}

fn field_data_type<'a>(schema: Option<&'a CollectionSchema>, field: &str) -> Option<&'a DataType> {
    let reference = crate::sql::ColumnIdentifierPath::parse(field).ok()?;
    schema?
        .fields
        .iter()
        .find(|entry| reference.matches_field_name(&entry.name))
        .map(|entry| &entry.data_type)
}

fn projected_document_matches(
    payload: &serde_json::Value,
    filter: &ProjectedDocumentFilter,
) -> bool {
    payload
        .as_object()
        .and_then(|object| projected_field_value(object, &filter.field))
        .map(json_to_value)
        .is_some_and(|value| value == filter.value)
}

/// Reads a canonical SQL column reference from a document payload, preserving
/// exact matching for delimited final components.
pub(crate) fn projected_field_value<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Option<&'a serde_json::Value> {
    let column = crate::sql::ColumnIdentifierPath::parse(field).ok()?;
    object.get(field).or_else(|| {
        column
            .row_lookup_candidates()
            .iter()
            .find_map(|candidate| object.get(candidate))
    })
}

fn json_to_value(value: &serde_json::Value) -> Value {
    if value.is_null() {
        return Value::Null;
    }
    if let Some(v) = value.as_str() {
        return Value::String(v.to_string());
    }
    if let Some(v) = value.as_bool() {
        return Value::Bool(v);
    }
    if let Some(v) = value.as_i64() {
        return Value::Int64(v);
    }
    if let Some(v) = value.as_u64().and_then(|v| i64::try_from(v).ok()) {
        return Value::Int64(v);
    }
    if let Some(v) = value.as_f64() {
        return Value::Float64(v);
    }
    Value::Json(value.clone())
}

fn json_to_typed_value(value: &serde_json::Value, data_type: &DataType) -> Value {
    if matches!(data_type, DataType::Json) && crate::types::json::requires_document_carrier(value) {
        return Value::Json(value.clone());
    }
    if let DataType::Vector(dimensions) = data_type {
        if let Some(values) = value.as_array() {
            if values.len() == *dimensions {
                let vector_values = values
                    .iter()
                    .map(|value| value.as_f64().and_then(parse_f64_to_f32))
                    .collect::<Option<Vec<_>>>();
                if let Some(vector_values) = vector_values {
                    return Value::Vector(Vector::new(vector_values));
                }
            }
        }
    }

    json_to_value(value)
}

fn parse_f64_to_f32(value: f64) -> Option<f32> {
    value.to_string().parse::<f32>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Cassie;
    use crate::types::{DataType, FieldSchema, Schema, Value};
    use uuid::Uuid;

    fn data_dir(label: &str) -> String {
        let mut dir = std::env::temp_dir();
        dir.push(format!("cassie-scan-{}-{}", label, Uuid::new_v4()));
        dir.to_string_lossy().to_string()
    }

    #[test]
    fn should_build_projected_rows_without_eager_lookup() {
        // Arrange
        let path = data_dir("projected-lazy-lookup");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let collection = "scan_projected_lazy_lookup";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .expect("create collection");
        cassie.register_collection(collection, schema);
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .expect("put document");

        // Act
        let controls = QueryExecutionControls::from_limits(
            &cassie.runtime.limits(),
            std::time::Instant::now(),
        );
        let fields = ["title".to_string()];
        let batches = scan_projected_filtered(
            &cassie,
            None,
            &ProjectedFilteredScanRequest {
                collection,
                fields: &fields,
                limit: None,
                document_filter: None,
                column_filter: None,
                controls: &controls,
            },
        )
        .expect("scan projected");

        // Assert
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len(), 1);
        assert!(!batches[0][0].lookup_initialized());
        // Entries are [_id, title]: `push_row_identity` always pushes the
        // reserved internal identity (`_id`) first, then the requested
        // `title` field.
        assert_eq!(
            batches[0][0].entries()[1].1,
            Value::String("alpha".to_string())
        );

        let _ = std::fs::remove_dir_all(path);
    }
}
