use crate::app::{Cassie, CassieSession};
use crate::catalog::{CollectionSchema, IndexKind};
use crate::executor::batch::{Batch, BatchRow, DEFAULT_BATCH_SIZE};
use crate::midge::adapter::RowFilter;
use crate::midge::adapter::{ColumnBatchScanDecision, ColumnBatchScanFilter, DocumentRef};
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

pub(crate) fn scan(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    collection: &str,
) -> Result<Vec<Batch>, crate::executor::QueryError> {
    let document_batches = cassie
        .scan_documents_batched_for_session(session, collection, DEFAULT_BATCH_SIZE)
        .map_err(|error| {
            cassie.runtime.record_storage_access("data", false, false);
            crate::executor::QueryError::General(error.to_string())
        })?;
    cassie.runtime.record_storage_access("data", false, true);
    let schema = cassie.catalog.get_schema(collection);

    Ok(document_batches_to_rows(
        cassie,
        document_batches,
        schema.as_ref(),
    ))
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
    let has_session_changes = session
        .map(|session| !session.collection_changes(collection).is_empty())
        .unwrap_or(false);
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

    let workers = worker_limit.min(document_batches.len());
    let mut indexed = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(document_batches.len());
        for (index, documents) in document_batches.into_iter().enumerate() {
            handles.push(scope.spawn(move || (index, document_batch_to_rows(documents, schema))));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().expect("parallel scan worker"))
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
                        let value = obj
                            .get(&field.name)
                            .map(|value| json_to_typed_value(value, &field.data_type))
                            .unwrap_or(Value::Null);
                        row.push((field.name.clone(), value));
                        seen.insert(field.name.clone());
                    }
                    for (k, v) in obj.iter() {
                        if !seen.contains(k) {
                            row.push((k.clone(), json_to_value(v)));
                        }
                    }
                } else {
                    for (k, v) in obj.iter() {
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

    let workers = worker_limit.min(document_batches.len());
    let mut indexed = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(document_batches.len());
        for (index, documents) in document_batches.into_iter().enumerate() {
            handles.push(scope.spawn(move || {
                (
                    index,
                    projected_document_batch_to_rows(documents, fields, document_filter, schema),
                )
            }));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().expect("parallel projected scan worker"))
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
                .map(|filter| projected_document_matches(&document.payload, filter))
                .unwrap_or(true)
        })
        .map(|document| {
            let mut row = Vec::with_capacity(fields.len() + 1);
            row.push(("id".to_string(), Value::String(document.id)));
            let object = document.payload.as_object();
            for field in fields {
                let value = object
                    .and_then(|object| projected_field_value(object, field))
                    .map(|value| {
                        field_data_type(schema, field)
                            .map(|data_type| json_to_typed_value(value, data_type))
                            .unwrap_or_else(|| json_to_value(value))
                    })
                    .unwrap_or(Value::Null);
                row.push((field.clone(), value));
            }
            BatchRow::from_projected_values(row)
        })
        .collect::<Batch>()
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
                    .map(|value| value.as_f64().map(|value| value as f32))
                    .collect::<Option<Vec<_>>>();
                if let Some(vector_values) = vector_values {
                    return Value::Vector(Vector::new(vector_values));
                }
            }
        }
    }

    json_to_value(value)
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
