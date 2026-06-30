use super::{
    batch, catalog, ensure_temp_budget, filter, projection, sort, BatchRow, BinaryOp, Cassie,
    CassieSession, CollectionSchema, DataType, Expr, FunctionMeta, HashMap, LogicalPlan,
    QueryError, QueryExecutionControls, QuerySource, RowDecode, Value,
};
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
    if session.is_some_and(|session| !session.collection_changes(&spec.collection).is_empty()) {
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
    let scan_fields = scan_fields_with_timestamp(&spec.scan_fields, &timestamp_field);

    let schema = cassie.catalog.get_schema(&spec.collection);
    let range = plan
        .filter
        .as_ref()
        .map(|filter| timestamp_range_from_filter(filter, &timestamp_field))
        .unwrap_or_default();
    let mut indexed_total_buckets = None;
    let mut documents = time_series_documents(
        cassie,
        &spec.collection,
        &scan_fields,
        &index,
        &range,
        &mut indexed_total_buckets,
    )?;
    let total_buckets = indexed_total_buckets
        .unwrap_or_else(|| bucket_keys(documents.as_slice(), &timestamp_field, &index).len());
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
        );
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

    if let Some((offset, limit)) = batch_window(plan) {
        batches = batch::slice_batches(batches, offset, limit);
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

fn scan_fields_with_timestamp(scan_fields: &[String], timestamp_field: &str) -> Vec<String> {
    let mut fields = scan_fields.to_vec();
    if !fields
        .iter()
        .any(|field| field.eq_ignore_ascii_case(timestamp_field))
    {
        fields.push(timestamp_field.to_string());
    }
    fields
}

fn time_series_documents(
    cassie: &Cassie,
    collection: &str,
    scan_fields: &[String],
    index: &catalog::IndexMeta,
    range: &TimestampRange,
    indexed_total_buckets: &mut Option<usize>,
) -> Result<Vec<DocumentRef>, QueryError> {
    match cassie.midge.scan_time_series_index(index) {
        Ok(report) if report.hits.is_empty() => {
            cassie
                .runtime
                .record_time_series_fallback("missing-bucket-metadata");
            scan_row_backed_documents(cassie, collection, scan_fields)
        }
        Ok(report) => {
            *indexed_total_buckets = Some(hit_bucket_keys(report.hits.as_slice()).len());
            let hits = prune_hits_for_range(report.hits, range);
            if hits.is_empty() {
                cassie.runtime.record_time_series_bucket_native_hit();
                return Ok(Vec::new());
            }
            let documents = cassie
                .midge
                .scan_time_series_hit_documents(collection, &hits, scan_fields)
                .map_err(|error| QueryError::General(error.to_string()))?;
            if documents.is_empty() {
                cassie
                    .runtime
                    .record_time_series_fallback("stale-bucket-metadata");
                scan_row_backed_documents(cassie, collection, scan_fields)
            } else {
                cassie.runtime.record_time_series_bucket_native_hit();
                Ok(documents)
            }
        }
        Err(error) => {
            let reason = if matches!(error, crate::app::CassieError::Unsupported(_)) {
                "unsupported-bucket-width"
            } else {
                "corrupt-bucket-metadata"
            };
            cassie.runtime.record_time_series_fallback(reason);
            scan_row_backed_documents(cassie, collection, scan_fields)
        }
    }
}

fn batch_window(plan: &LogicalPlan) -> Option<(usize, Option<usize>)> {
    let offset = plan.offset.and_then(non_negative_usize).unwrap_or_default();
    let limit = plan.limit.and_then(non_negative_usize);
    (offset > 0 || limit.is_some()).then_some((offset, limit))
}

fn non_negative_usize(value: i64) -> Option<usize> {
    usize::try_from(value.max(0)).ok()
}

fn prune_hits_for_range(
    hits: Vec<crate::midge::adapter::time_series_indexes::TimeSeriesIndexScanHit>,
    range: &TimestampRange,
) -> Vec<crate::midge::adapter::time_series_indexes::TimeSeriesIndexScanHit> {
    hits.into_iter()
        .filter(|hit| {
            range
                .lower
                .as_ref()
                .is_none_or(|lower| hit.timestamp.as_str() >= lower.as_str())
                && range
                    .upper
                    .as_ref()
                    .is_none_or(|upper| hit.timestamp.as_str() <= upper.as_str())
        })
        .collect()
}

#[derive(Debug, Clone, Default)]
struct TimestampRange {
    lower: Option<String>,
    upper: Option<String>,
}

fn timestamp_range_from_filter(expr: &Expr, timestamp_field: &str) -> TimestampRange {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => {
            let left = timestamp_range_from_filter(left, timestamp_field);
            let right = timestamp_range_from_filter(right, timestamp_field);
            TimestampRange {
                lower: max_bound(left.lower, right.lower),
                upper: min_bound(left.upper, right.upper),
            }
        }
        Expr::Binary { left, op, right } => {
            comparison_timestamp_range(left, op, right, timestamp_field).unwrap_or_default()
        }
        Expr::Between {
            expr,
            low,
            high,
            negated: false,
        } if is_timestamp_column(expr, timestamp_field) => TimestampRange {
            lower: timestamp_literal(low),
            upper: timestamp_literal(high),
        },
        _ => TimestampRange::default(),
    }
}

fn comparison_timestamp_range(
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
    timestamp_field: &str,
) -> Option<TimestampRange> {
    if is_timestamp_column(left, timestamp_field) {
        let value = timestamp_literal(right)?;
        return match op {
            BinaryOp::Gt | BinaryOp::Gte => Some(TimestampRange {
                lower: Some(value),
                upper: None,
            }),
            BinaryOp::Lt | BinaryOp::Lte => Some(TimestampRange {
                lower: None,
                upper: Some(value),
            }),
            _ => None,
        };
    }
    if is_timestamp_column(right, timestamp_field) {
        let value = timestamp_literal(left)?;
        return match op {
            BinaryOp::Gt | BinaryOp::Gte => Some(TimestampRange {
                lower: None,
                upper: Some(value),
            }),
            BinaryOp::Lt | BinaryOp::Lte => Some(TimestampRange {
                lower: Some(value),
                upper: None,
            }),
            _ => None,
        };
    }
    None
}

fn is_timestamp_column(expr: &Expr, timestamp_field: &str) -> bool {
    matches!(expr, Expr::Column(field) if field.eq_ignore_ascii_case(timestamp_field))
}

fn timestamp_literal(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StringLiteral(value) => Some(value.clone()),
        _ => None,
    }
}

fn max_bound(left: Option<String>, right: Option<String>) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn min_bound(left: Option<String>, right: Option<String>) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn hit_bucket_keys(
    hits: &[crate::midge::adapter::time_series_indexes::TimeSeriesIndexScanHit],
) -> std::collections::BTreeSet<String> {
    hits.iter().map(|hit| hit.bucket_key.clone()).collect()
}

fn scan_row_backed_documents(
    cassie: &Cassie,
    collection: &str,
    scan_fields: &[String],
) -> Result<Vec<DocumentRef>, QueryError> {
    cassie
        .midge
        .scan_rows_for_rebuild(
            collection,
            RowDecode::ProjectedHistorical(scan_fields.to_vec()),
        )
        .map_err(|error| QueryError::General(error.to_string()))
}

fn selected_time_series_index(cassie: &Cassie, plan: &LogicalPlan) -> Option<catalog::IndexMeta> {
    let QuerySource::Collection(collection) = &plan.source else {
        return None;
    };
    let indexes = cassie.catalog.list_indexes(collection);
    let cardinality_stats =
        std::collections::HashMap::<String, crate::catalog::CollectionCardinalityStats>::new();
    let physical = crate::planner::physical::build_with_indexes(
        plan.clone(),
        indexes.as_slice(),
        &cardinality_stats,
    );
    let selected = physical.read.selected_index?;
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
            .map_or(Value::Null, |value| typed_value(value, schema, field));
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
        Some(DataType::Int | DataType::BigInt) => value
            .as_i64()
            .map_or_else(|| json_to_query_value(value), Value::Int64),
        Some(DataType::Float) => value
            .as_f64()
            .map_or_else(|| json_to_query_value(value), Value::Float64),
        Some(DataType::Boolean) => value
            .as_bool()
            .map_or_else(|| json_to_query_value(value), Value::Bool),
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
