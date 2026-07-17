use crate::app::{Cassie, CassieSession, SessionRowCursor};
use crate::catalog::{CollectionSchema, IndexKind};
use crate::executor::batch::{Batch, BatchRow, BatchStream, DEFAULT_BATCH_SIZE};
use crate::midge::adapter::RowFilter;
use crate::midge::adapter::{
    ColumnBatchScanDecision, ColumnBatchScanFilter, DocumentRef, RowDecode,
};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::{DataType, Value, Vector};
use std::collections::HashSet;
use std::time::Duration;

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
    previous_page_memory: Vec<QueryMemoryReservation>,
}

impl BatchStream for ProjectedScanStream<'_> {
    fn next_batch(&mut self) -> Result<Option<Batch>, crate::executor::QueryError> {
        self.previous_page_memory.clear();
        if self.remaining == 0 {
            return Ok(None);
        }
        let batch_size = self.remaining.min(DEFAULT_BATCH_SIZE);
        let accounted_documents = self
            .cursor
            .next_accounted_documents(&self.cassie.midge, batch_size, self.controls)
            .map_err(crate::executor::QueryError::from)?;
        if accounted_documents.is_empty() {
            return Ok(None);
        }
        let mut documents = Vec::with_capacity(accounted_documents.len());
        for document in accounted_documents {
            let (document, reservation) = document.into_parts();
            documents.push(document);
            self.previous_page_memory.push(reservation);
        }
        self.remaining = self.remaining.saturating_sub(documents.len());
        Ok(Some(projected_document_batch_to_rows(
            documents,
            &self.fields,
            None,
            self.schema.as_ref(),
        )))
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
    if cassie.runtime.limits().parallel_scan_workers.max(1) > 1 {
        return Ok(None);
    }
    let Some(cursor) = cassie
        .open_session_row_cursor(
            session,
            collection,
            RowDecode::ProjectedHistorical(fields.to_vec()),
            controls,
        )
        .map_err(crate::executor::QueryError::from)?
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
        previous_page_memory: Vec::new(),
    }))
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
    if cassie.runtime.limits().parallel_scan_workers.max(1) == 1 {
        if let Some(mut cursor) = cassie
            .open_session_row_cursor(session, collection, RowDecode::Full, controls)
            .map_err(crate::executor::QueryError::from)?
        {
            let schema = cassie.catalog.get_schema(collection);
            let mut remaining = limit.unwrap_or(usize::MAX);
            let mut batches = Vec::new();
            let mut row_memory = Vec::new();
            while remaining > 0 {
                let accounted = cursor
                    .next_accounted_documents(
                        &cassie.midge,
                        remaining.min(DEFAULT_BATCH_SIZE),
                        controls,
                    )
                    .map_err(crate::executor::QueryError::from)?;
                if accounted.is_empty() {
                    break;
                }
                let retained_bytes = accounted
                    .iter()
                    .map(crate::midge::adapter::AccountedDocument::accounted_bytes)
                    .sum();
                let reservation = controls.reserve_query_memory(retained_bytes)?;
                let documents = accounted
                    .into_iter()
                    .map(|document| document.into_parts().0)
                    .collect();
                let batch = document_batch_to_rows(documents, schema.as_ref());
                remaining = remaining.saturating_sub(batch.len());
                batches.push(batch);
                row_memory.push(reservation);
            }
            cassie.runtime.record_parallel_scan_fallback();
            cassie.runtime.record_storage_access("data", false, true);
            let rows = batches.iter().map(Vec::len).sum::<usize>();
            let fields = schema.as_ref().map_or(0, |schema| schema.fields.len());
            cassie
                .runtime
                .record_read_path_collection_scan(collection, fields, rows);
            drop(row_memory);
            return Ok(batches);
        }
    }
    let document_batches = cassie
        .scan_documents_batched_for_session_limit(session, collection, DEFAULT_BATCH_SIZE, limit)
        .map_err(|error| {
            cassie.runtime.record_storage_access("data", false, false);
            crate::executor::QueryError::General(error.to_string())
        })?;
    cassie.runtime.record_storage_access("data", false, true);
    let schema = cassie.catalog.get_schema(collection);

    let batches = document_batches_to_rows(cassie, document_batches, schema.as_ref());
    let _memory = controls.reserve_query_memory(
        batches
            .iter()
            .flatten()
            .map(|row| serde_json::to_vec(row.entries()).map_or(0, |bytes| bytes.len()))
            .sum(),
    )?;
    let rows = batches.iter().map(Vec::len).sum::<usize>();
    let fields = schema
        .as_ref()
        .map(|schema| schema.fields.len())
        .unwrap_or_default();
    cassie
        .runtime
        .record_read_path_collection_scan(collection, fields, rows);
    Ok(batches)
}

pub(crate) fn scan_projected_filtered(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    collection: &str,
    fields: &[String],
    limit: Option<usize>,
    document_filter: Option<&ProjectedDocumentFilter>,
    column_filter: Option<&ColumnBatchScanFilter>,
) -> Result<Vec<Batch>, crate::executor::QueryError> {
    scan_projected_filtered_with_timings(
        cassie,
        session,
        collection,
        fields,
        limit,
        document_filter,
        column_filter,
    )
    .map(|(batches, _)| batches)
}

pub(crate) fn scan_projected_filtered_with_timings(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    collection: &str,
    fields: &[String],
    limit: Option<usize>,
    document_filter: Option<&ProjectedDocumentFilter>,
    column_filter: Option<&ColumnBatchScanFilter>,
) -> Result<(Vec<Batch>, ScanTimings), crate::executor::QueryError> {
    let storage_filter = document_filter.and_then(row_filter_from_projected_filter);
    let has_session_changes =
        session.is_some_and(|session| session.has_collection_changes(collection));
    if !has_session_changes {
        match cassie.midge.scan_column_batch_projected_rows(
            collection,
            DEFAULT_BATCH_SIZE,
            fields,
            storage_filter.as_ref(),
            column_filter,
            limit,
        ) {
            Ok(ColumnBatchScanDecision::Hit(outcome)) => {
                cassie.runtime.record_storage_access("data", false, true);
                let schema = cassie.catalog.get_schema(collection);
                let mut timings = ScanTimings {
                    scan: outcome.timings.scan,
                    row_decode: outcome.timings.row_decode,
                };
                let materialize_started = std::time::Instant::now();
                let batches = projected_document_batches_to_rows(
                    cassie,
                    outcome.batches,
                    fields,
                    document_filter,
                    schema.as_ref(),
                );
                timings.scan += materialize_started.elapsed();
                let rows = batches.iter().map(Vec::len).sum::<usize>();
                cassie.runtime.record_column_batch_scan(
                    rows,
                    outcome.compressed_bytes,
                    outcome.uncompressed_bytes,
                    outcome.skipped_segments,
                    outcome.decoded_columns,
                );
                return Ok((batches, timings));
            }
            Ok(ColumnBatchScanDecision::Fallback(reason)) => {
                if has_covering_column_index(cassie, collection, fields) {
                    if reason.is_decode_fallback() {
                        cassie
                            .runtime
                            .record_column_batch_decode_fallback_with_reason(reason.as_str());
                    } else {
                        cassie.runtime.record_column_batch_fallback(reason.as_str());
                    }
                }
            }
            Err(error) => {
                cassie.runtime.record_column_batch_fallback("error");
                cassie.runtime.record_storage_access("data", false, false);
                return Err(crate::executor::QueryError::General(error.to_string()));
            }
        }
    }
    let (document_batches, raw_timings) = cassie
        .scan_projected_documents_batched_for_session_with_filter_and_timings(
            session,
            collection,
            DEFAULT_BATCH_SIZE,
            fields,
            storage_filter.as_ref(),
            limit,
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
    let schema = cassie.catalog.get_schema(collection);
    let batches = projected_document_batches_to_rows(
        cassie,
        document_batches,
        fields,
        document_filter,
        schema.as_ref(),
    );
    if has_session_changes && has_covering_column_index(cassie, collection, fields) {
        let rows = batches.iter().map(Vec::len).sum::<usize>();
        cassie
            .runtime
            .record_column_batch_row_blob_fallback(rows, "session-changes");
    }
    timings.scan += materialize_started.elapsed();

    Ok((batches, timings))
}

fn has_covering_column_index(cassie: &Cassie, collection: &str, fields: &[String]) -> bool {
    let wanted = fields
        .iter()
        .filter(|field| !field.eq_ignore_ascii_case("id") && !field.eq_ignore_ascii_case("_id"))
        .map(|field| field.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    !wanted.is_empty()
        && cassie
            .catalog
            .list_indexes(collection)
            .into_iter()
            .any(|index| {
                index.kind == IndexKind::Column
                    && wanted.iter().all(|field| {
                        index
                            .normalized_fields()
                            .iter()
                            .any(|candidate| candidate.eq_ignore_ascii_case(field))
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

fn document_batches_to_rows(
    cassie: &Cassie,
    document_batches: Vec<Vec<DocumentRef>>,
    schema: Option<&CollectionSchema>,
) -> Vec<Batch> {
    let worker_limit = cassie.runtime.limits().parallel_scan_workers.max(1);
    if worker_limit == 1 || document_batches.len() < 2 {
        cassie.runtime.record_parallel_scan_fallback();
        return document_batches
            .into_iter()
            .map(|documents| document_batch_to_rows(documents, schema))
            .collect();
    }

    let Some(worker_guard) = cassie.runtime.try_acquire_operator_workers(worker_limit) else {
        cassie.runtime.record_parallel_scan_fallback();
        return document_batches
            .into_iter()
            .map(|documents| document_batch_to_rows(documents, schema))
            .collect();
    };
    let workers = worker_guard.workers().min(document_batches.len());
    let partitions = partition_document_batches(document_batches, workers);
    let mut indexed = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for partition in partitions {
            handles.push(scope.spawn(move || {
                partition
                    .into_iter()
                    .map(|(index, documents)| (index, document_batch_to_rows(documents, schema)))
                    .collect::<Vec<_>>()
            }));
        }
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("parallel scan worker"))
            .collect::<Vec<_>>()
    });
    indexed.sort_by_key(|(index, _)| *index);
    let rows = indexed.iter().map(|(_, batch)| batch.len()).sum::<usize>();
    cassie
        .runtime
        .record_parallel_scan(workers, indexed.len(), rows);
    indexed.into_iter().map(|(_, batch)| batch).collect()
}

fn document_batch_to_rows(documents: Vec<DocumentRef>, schema: Option<&CollectionSchema>) -> Batch {
    documents
        .into_iter()
        .map(|document| {
            let mut row = Vec::new();
            row.push(("id".to_string(), Value::String(document.id)));
            if let Some(obj) = document.payload.as_object() {
                if let Some(schema) = schema.as_ref() {
                    let mut seen = HashSet::new();
                    for field in &schema.fields {
                        let value = obj.get(&field.name).map_or(Value::Null, |value| {
                            json_to_typed_value(value, &field.data_type)
                        });
                        row.push((field.name.clone(), value));
                        seen.insert(field.name.clone());
                    }
                    for (k, v) in obj {
                        if !seen.contains(k) {
                            row.push((k.clone(), json_to_value(v)));
                        }
                    }
                } else {
                    for (k, v) in obj {
                        row.push((k.clone(), json_to_value(v)));
                    }
                }
            }
            BatchRow::new(row)
        })
        .collect::<Batch>()
}

fn projected_document_batches_to_rows(
    cassie: &Cassie,
    document_batches: Vec<Vec<DocumentRef>>,
    fields: &[String],
    document_filter: Option<&ProjectedDocumentFilter>,
    schema: Option<&CollectionSchema>,
) -> Vec<Batch> {
    let worker_limit = cassie.runtime.limits().parallel_scan_workers.max(1);
    if worker_limit == 1 || document_batches.len() < 2 {
        cassie.runtime.record_parallel_scan_fallback();
        return document_batches
            .into_iter()
            .filter_map(|documents| {
                let rows =
                    projected_document_batch_to_rows(documents, fields, document_filter, schema);
                (!rows.is_empty()).then_some(rows)
            })
            .collect();
    }

    let Some(worker_guard) = cassie.runtime.try_acquire_operator_workers(worker_limit) else {
        cassie.runtime.record_parallel_scan_fallback();
        return document_batches
            .into_iter()
            .filter_map(|documents| {
                let rows =
                    projected_document_batch_to_rows(documents, fields, document_filter, schema);
                (!rows.is_empty()).then_some(rows)
            })
            .collect();
    };
    let workers = worker_guard.workers().min(document_batches.len());
    let partitions = partition_document_batches(document_batches, workers);
    let mut indexed = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for partition in partitions {
            handles.push(scope.spawn(move || {
                partition
                    .into_iter()
                    .map(|(index, documents)| {
                        (
                            index,
                            projected_document_batch_to_rows(
                                documents,
                                fields,
                                document_filter,
                                schema,
                            ),
                        )
                    })
                    .collect::<Vec<_>>()
            }));
        }
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("parallel projected scan worker"))
            .collect::<Vec<_>>()
    });
    indexed.sort_by_key(|(index, _)| *index);
    let rows = indexed.iter().map(|(_, batch)| batch.len()).sum::<usize>();
    cassie
        .runtime
        .record_parallel_scan(workers, indexed.len(), rows);
    indexed
        .into_iter()
        .filter_map(|(_, batch)| (!batch.is_empty()).then_some(batch))
        .collect()
}

fn partition_document_batches(
    document_batches: Vec<Vec<DocumentRef>>,
    workers: usize,
) -> Vec<Vec<(usize, Vec<DocumentRef>)>> {
    let mut partitions = (0..workers).map(|_| Vec::new()).collect::<Vec<_>>();
    for (index, documents) in document_batches.into_iter().enumerate() {
        partitions[index % workers].push((index, documents));
    }
    partitions
}

fn projected_document_batch_to_rows(
    documents: Vec<DocumentRef>,
    fields: &[String],
    document_filter: Option<&ProjectedDocumentFilter>,
    schema: Option<&CollectionSchema>,
) -> Batch {
    documents
        .into_iter()
        .filter(|document| {
            document_filter
                .is_none_or(|filter| projected_document_matches(&document.payload, filter))
        })
        .map(|document| projected_document_to_row(document, fields, schema))
        .collect::<Batch>()
}

pub(crate) fn projected_document_to_row(
    document: DocumentRef,
    fields: &[String],
    schema: Option<&CollectionSchema>,
) -> BatchRow {
    let mut row = Vec::with_capacity(fields.len() + 1);
    row.push(("id".to_string(), Value::String(document.id)));
    let object = document.payload.as_object();
    for field in fields {
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
    BatchRow::from_projected_values(row)
}

fn field_data_type<'a>(schema: Option<&'a CollectionSchema>, field: &str) -> Option<&'a DataType> {
    schema?
        .fields
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(field))
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

fn projected_field_value<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Option<&'a serde_json::Value> {
    object.get(field).or_else(|| {
        object
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(field))
            .map(|(_, value)| value)
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
        std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
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
        let batches = scan_projected_filtered(
            &cassie,
            None,
            collection,
            &["title".to_string()],
            None,
            None,
            None,
        )
        .expect("scan projected");

        // Assert
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len(), 1);
        assert!(!batches[0][0].lookup_initialized());
        assert_eq!(
            batches[0][0].entries()[1].1,
            Value::String("alpha".to_string())
        );

        let _ = std::fs::remove_dir_all(path);
    }
}
