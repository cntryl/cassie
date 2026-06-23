use super::*;
use crate::midge::adapter::DocumentRef;

use super::projected_read::json_to_query_value;

pub(super) fn try_execute_time_series_read(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some(spec) = super::projected_read::projected_filtered_read_spec(plan) else {
        return Ok(None);
    };
    let Some(index) = selected_time_series_index(cassie, plan) else {
        return Ok(None);
    };
    if session
        .map(|session| !session.collection_changes(&spec.collection).is_empty())
        .unwrap_or(false)
    {
        cassie
            .runtime
            .record_time_series_fallback("session-changes");
        return Ok(None);
    }
    if plan.filter.is_none() {
        cassie
            .runtime
            .record_time_series_fallback("missing-range-filter");
        return Ok(None);
    }

    let timestamp_field = index.primary_field();
    let mut scan_fields = spec.scan_fields.clone();
    if !scan_fields
        .iter()
        .any(|field| field.eq_ignore_ascii_case(&timestamp_field))
    {
        scan_fields.push(timestamp_field.clone());
    }

    let schema = cassie.catalog.get_schema(&spec.collection);
    let mut documents = match cassie.midge.scan_time_series_index(&index) {
        Ok(report) if report.hits.is_empty() => {
            cassie
                .runtime
                .record_time_series_fallback("missing-bucket-metadata");
            scan_row_backed_documents(cassie, &spec.collection, scan_fields.clone())?
        }
        Ok(report) => {
            let mut documents = Vec::with_capacity(report.hits.len());
            for hit in report.hits {
                if let Some(document) = cassie
                    .midge
                    .get_document(&spec.collection, &hit.id)
                    .map_err(|error| QueryError::General(error.to_string()))?
                {
                    documents.push(document);
                }
            }
            if documents.is_empty() {
                cassie
                    .runtime
                    .record_time_series_fallback("stale-bucket-metadata");
                scan_row_backed_documents(cassie, &spec.collection, scan_fields.clone())?
            } else {
                cassie.runtime.record_time_series_bucket_native_hit();
                documents
            }
        }
        Err(error) => {
            let reason = if matches!(error, crate::app::CassieError::Unsupported(_)) {
                "unsupported-bucket-width"
            } else {
                "corrupt-bucket-metadata"
            };
            cassie.runtime.record_time_series_fallback(reason);
            scan_row_backed_documents(cassie, &spec.collection, scan_fields.clone())?
        }
    };
    let total_buckets = bucket_keys(documents.as_slice(), &timestamp_field, &index).len();
    documents.sort_by(|left, right| {
        timestamp_sort_key(&left.payload, &timestamp_field)
            .cmp(&timestamp_sort_key(&right.payload, &timestamp_field))
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut rows = documents
        .into_iter()
        .map(|document| document_to_row(document, scan_fields.as_slice(), schema.as_ref()))
        .collect::<Vec<_>>();
    let before_filter_buckets = row_bucket_keys(rows.as_slice(), &timestamp_field, &index).len();
    let mut batches = batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE);
    ensure_temp_budget(controls, &batches)?;

    if let Some(filter_expr) = &plan.filter {
        batches =
            filter::filter_batches(batches, filter_expr, params, None, user_functions, session)?;
        ensure_temp_budget(controls, &batches)?;
    }

    rows = batch::flatten_batches(batches);
    let scanned_buckets = row_bucket_keys(rows.as_slice(), &timestamp_field, &index).len();
    let skipped_buckets = total_buckets
        .max(before_filter_buckets)
        .saturating_sub(scanned_buckets);
    let mut batches = batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE);

    if !plan.order.is_empty() {
        batches = sort::sort_batches(
            batches,
            &plan.order,
            &plan.projection,
            params,
            None,
            user_functions,
            session,
        )?;
        ensure_temp_budget(controls, &batches)?;
    }

    batches = projection::project_batches(
        batches,
        &plan.projection,
        params,
        None,
        user_functions,
        session,
    )?;
    ensure_temp_budget(controls, &batches)?;

    if let Some(offset) = plan.offset {
        let offset = offset.max(0) as usize;
        let limit = plan.limit.map(|value| value.max(0) as usize);
        batches = batch::slice_batches(batches, offset, limit);
    } else if let Some(limit) = plan.limit {
        let limit = limit.max(0) as usize;
        batches = batch::slice_batches(batches, 0, Some(limit));
    }

    let rows = batch::flatten_batches(batches);
    cassie.runtime.record_time_series_scan(
        index.name,
        rows.len(),
        scanned_buckets,
        skipped_buckets,
    );
    Ok(Some(rows))
}

fn scan_row_backed_documents(
    cassie: &Cassie,
    collection: &str,
    scan_fields: Vec<String>,
) -> Result<Vec<DocumentRef>, QueryError> {
    cassie
        .midge
        .scan_rows_for_rebuild(collection, RowDecode::Projected(scan_fields))
        .map_err(|error| QueryError::General(error.to_string()))
}

fn selected_time_series_index(cassie: &Cassie, plan: &LogicalPlan) -> Option<catalog::IndexMeta> {
    let QuerySource::Collection(collection) = &plan.source else {
        return None;
    };
    let indexes = cassie.catalog.list_indexes(collection);
    let physical = crate::planner::physical::build_with_indexes(
        plan.clone(),
        indexes.clone(),
        &Default::default(),
    );
    let selected = physical.selected_index?;
    indexes
        .into_iter()
        .find(|index| index.name == selected && index.kind == catalog::IndexKind::TimeSeries)
}

fn document_to_row(
    document: DocumentRef,
    fields: &[String],
    schema: Option<&CollectionSchema>,
) -> BatchRow {
    let mut values = Vec::with_capacity(fields.len() + 1);
    values.push(("id".to_string(), Value::String(document.id)));
    for field in fields {
        let value = payload_field(&document.payload, field)
            .map(|value| typed_value(value, schema, field))
            .unwrap_or(Value::Null);
        values.push((field.clone(), value));
    }
    BatchRow::from_projected_values(values)
}

fn typed_value(value: &serde_json::Value, schema: Option<&CollectionSchema>, field: &str) -> Value {
    match schema
        .and_then(|schema| {
            schema
                .fields
                .iter()
                .find(|entry| entry.name.eq_ignore_ascii_case(field))
        })
        .map(|entry| &entry.data_type)
    {
        Some(DataType::Int) | Some(DataType::BigInt) => value
            .as_i64()
            .map(Value::Int64)
            .unwrap_or_else(|| json_to_query_value(value)),
        Some(DataType::Float) => value
            .as_f64()
            .map(Value::Float64)
            .unwrap_or_else(|| json_to_query_value(value)),
        Some(DataType::Boolean) => value
            .as_bool()
            .map(Value::Bool)
            .unwrap_or_else(|| json_to_query_value(value)),
        _ => json_to_query_value(value),
    }
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

fn timestamp_sort_key(payload: &serde_json::Value, field: &str) -> String {
    payload_field(payload, field)
        .and_then(|value| value.as_str().map(ToString::to_string))
        .unwrap_or_default()
}

fn bucket_keys(
    documents: &[DocumentRef],
    timestamp_field: &str,
    index: &catalog::IndexMeta,
) -> std::collections::BTreeSet<String> {
    documents
        .iter()
        .filter_map(|document| {
            payload_field(&document.payload, timestamp_field)
                .and_then(|value| value.as_str())
                .map(|value| bucket_key(value, index))
        })
        .collect()
}

fn row_bucket_keys(
    rows: &[BatchRow],
    timestamp_field: &str,
    index: &catalog::IndexMeta,
) -> std::collections::BTreeSet<String> {
    rows.iter()
        .filter_map(|row| {
            row.get(timestamp_field).and_then(|value| match value {
                Value::String(value) => Some(bucket_key(value, index)),
                _ => None,
            })
        })
        .collect()
}

fn bucket_key(timestamp: &str, index: &catalog::IndexMeta) -> String {
    let partition = index
        .options
        .get("partition_by")
        .cloned()
        .unwrap_or_else(|| "none".to_string());
    let width = index
        .options
        .get("bucket_width")
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    let time_part = if width.contains("hour") {
        timestamp.get(..13).unwrap_or(timestamp)
    } else if width.contains("day") {
        timestamp.get(..10).unwrap_or(timestamp)
    } else {
        timestamp
    };
    format!("{partition}:{time_part}")
}
