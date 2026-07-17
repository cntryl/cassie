use super::{
    batch, catalog, check_timeout, ensure_query_memory_budget, filter, projection, sort, BatchRow,
    BinaryOp, Cassie, CassieSession, CollectionSchema, DataType, Expr, FunctionMeta, HashMap,
    LogicalPlan, QueryError, QueryExecutionControls, QuerySource, RowDecode, Value,
};
use crate::midge::adapter::time_series_indexes::{
    TimeSeriesDocumentScanOutcome, TimeSeriesIndexScanOutcome,
};
use crate::midge::adapter::DocumentRef;
use time::{Duration as TimeDuration, OffsetDateTime};

use super::projected_read::json_to_query_value;

pub(super) fn try_execute_time_series_read(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    check_timeout(controls)?;
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
        .map(|filter| timestamp_range_from_filter(filter, &timestamp_field, params))
        .unwrap_or_default();
    let partition_key = partition_key_from_filter(
        plan.filter.as_ref(),
        index.options.get("partition_by").map(String::as_str),
        params,
    );
    let (lower_bucket_seconds, upper_bucket_seconds) = time_series_bucket_bounds(&index, &range);
    let scan_request = TimeSeriesScanRequest {
        collection: &spec.collection,
        scan_fields: &scan_fields,
        index: &index,
        range: &range,
        partition_key: partition_key.as_deref(),
        lower_bucket_seconds,
        upper_bucket_seconds,
    };
    let (documents, index_entries_scanned, row_point_fetches, indexed_total_buckets) =
        time_series_documents(cassie, &scan_request)?;
    let total_buckets = indexed_total_buckets
        .unwrap_or_else(|| bucket_keys(documents.as_slice(), &timestamp_field, &index).len());
    let context = TimeSeriesExecutionContext {
        cassie,
        session,
        plan,
        user_functions,
        params,
        controls,
        timestamp_field: &timestamp_field,
        scan_fields: &scan_fields,
        index: &index,
        schema: schema.as_ref(),
    };
    execute_time_series_rows(
        documents,
        total_buckets,
        index_entries_scanned,
        row_point_fetches,
        &context,
    )
}

struct TimeSeriesExecutionContext<'a> {
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    plan: &'a LogicalPlan,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    controls: &'a QueryExecutionControls,
    timestamp_field: &'a str,
    scan_fields: &'a [String],
    index: &'a catalog::IndexMeta,
    schema: Option<&'a CollectionSchema>,
}

fn execute_time_series_rows(
    mut documents: Vec<DocumentRef>,
    total_buckets: usize,
    index_entries_scanned: usize,
    row_point_fetches: usize,
    context: &TimeSeriesExecutionContext<'_>,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    check_timeout(context.controls)?;
    documents.sort_by(|left, right| {
        timestamp_sort_key(&left.payload, context.timestamp_field)
            .cmp(&timestamp_sort_key(&right.payload, context.timestamp_field))
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut rows = Vec::with_capacity(documents.len());
    for document in documents {
        check_timeout(context.controls)?;
        rows.push(document_to_row(
            document,
            context.scan_fields,
            context.schema,
        ));
    }
    let before_filter_buckets =
        row_bucket_keys(rows.as_slice(), context.timestamp_field, context.index).len();
    let mut batches = batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE);
    ensure_query_memory_budget(context.controls, &batches)?;

    if let Some(filter_expr) = &context.plan.filter {
        batches = filter::filter_batches(
            batches,
            filter_expr,
            context.params,
            None,
            context.user_functions,
            context.session,
        )?;
        ensure_query_memory_budget(context.controls, &batches)?;
    }

    rows = batch::flatten_batches(batches);
    let (scanned_buckets, skipped_buckets) = time_series_bucket_metrics(
        rows.as_slice(),
        context.timestamp_field,
        context.index,
        total_buckets,
        before_filter_buckets,
    );
    let mut batches = batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE);
    if !context.plan.order.is_empty() {
        let eval = sort::EvalInput {
            order: &context.plan.order,
            projection: &context.plan.projection,
            params: context.params,
            search_context: None,
            user_functions: context.user_functions,
            session: context.session,
        };
        batches = sort::sort_batches_with_controls(batches, &eval, context.controls)?;
        ensure_query_memory_budget(context.controls, &batches)?;
    }
    batches = projection::project_batches(
        batches,
        &context.plan.projection,
        context.params,
        None,
        context.user_functions,
        context.session,
    )?;
    ensure_query_memory_budget(context.controls, &batches)?;
    if let Some((offset, limit)) = batch_window(context.plan) {
        batches = batch::slice_batches(batches, offset, limit);
    }
    let rows = batch::flatten_batches(batches);
    context.cassie.runtime.record_time_series_scan(
        &context.index.name,
        rows.len(),
        scanned_buckets,
        skipped_buckets,
        index_entries_scanned,
        row_point_fetches,
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

fn time_series_bucket_metrics(
    rows: &[BatchRow],
    timestamp_field: &str,
    index: &catalog::IndexMeta,
    total_buckets: usize,
    before_filter_buckets: usize,
) -> (usize, usize) {
    let scanned_buckets = row_bucket_keys(rows, timestamp_field, index).len();
    let skipped_buckets = total_buckets
        .max(before_filter_buckets)
        .saturating_sub(scanned_buckets);
    (scanned_buckets, skipped_buckets)
}

struct TimeSeriesScanRequest<'a> {
    collection: &'a str,
    scan_fields: &'a [String],
    index: &'a catalog::IndexMeta,
    range: &'a TimestampRange,
    partition_key: Option<&'a str>,
    lower_bucket_seconds: Option<i64>,
    upper_bucket_seconds: Option<i64>,
}

fn time_series_documents(
    cassie: &Cassie,
    request: &TimeSeriesScanRequest<'_>,
) -> Result<(Vec<DocumentRef>, usize, usize, Option<usize>), QueryError> {
    match cassie.midge.scan_time_series_index(
        request.index,
        request.partition_key,
        request.lower_bucket_seconds,
        request.upper_bucket_seconds,
    ) {
        Ok(TimeSeriesIndexScanOutcome::Native(report)) => {
            let indexed_total_buckets = Some(hit_bucket_keys(report.hits.as_slice()).len());
            let hits = prune_hits_for_range(report.hits, request.range);
            if hits.is_empty() {
                cassie.runtime.record_time_series_bucket_native_hit();
                return Ok((Vec::new(), report.entries_scanned, 0, indexed_total_buckets));
            }
            let documents = cassie
                .midge
                .scan_time_series_hit_documents(request.index, &hits, request.scan_fields)
                .map_err(|error| QueryError::General(error.to_string()))?;
            match documents {
                TimeSeriesDocumentScanOutcome::Native(documents)
                    if cassie
                        .midge
                        .collection_generation(request.collection)
                        .map_err(|error| QueryError::General(error.to_string()))?
                        == report.generation =>
                {
                    cassie.runtime.record_time_series_bucket_native_hit();
                    Ok((
                        documents,
                        report.entries_scanned,
                        hits.len(),
                        indexed_total_buckets,
                    ))
                }
                TimeSeriesDocumentScanOutcome::Native(_) => {
                    cassie
                        .runtime
                        .record_time_series_fallback("stale-bucket-metadata");
                    scan_row_backed_documents(cassie, request.collection, request.scan_fields)
                        .map(|documents| (documents, report.entries_scanned, 0, None))
                }
                TimeSeriesDocumentScanOutcome::Fallback(reason) => {
                    cassie.runtime.record_time_series_fallback(reason);
                    scan_row_backed_documents(cassie, request.collection, request.scan_fields)
                        .map(|documents| (documents, report.entries_scanned, 0, None))
                }
            }
        }
        Ok(TimeSeriesIndexScanOutcome::Fallback(reason)) => {
            cassie.runtime.record_time_series_fallback(reason);
            scan_row_backed_documents(cassie, request.collection, request.scan_fields)
                .map(|documents| (documents, 0, 0, None))
        }
        Err(error) => {
            let reason = if matches!(error, crate::app::CassieError::Unsupported(_)) {
                "unsupported-bucket-width"
            } else {
                "corrupt-bucket-metadata"
            };
            cassie.runtime.record_time_series_fallback(reason);
            scan_row_backed_documents(cassie, request.collection, request.scan_fields)
                .map(|documents| (documents, 0, 0, None))
        }
    }
}

fn batch_window(plan: &LogicalPlan) -> Option<(usize, Option<usize>)> {
    let offset = plan.offset.and_then(non_negative_usize).unwrap_or_default();
    let limit = plan.limit.and_then(non_negative_usize);
    (offset > 0 || limit.is_some()).then_some((offset, limit))
}

fn time_series_bucket_bounds(
    index: &catalog::IndexMeta,
    range: &TimestampRange,
) -> (Option<i64>, Option<i64>) {
    let Some(width) = time_series_bucket_width(index) else {
        return (None, None);
    };
    let lower = range.lower.as_deref().and_then(|value| {
        OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
            .ok()
            .map(|timestamp| timestamp.unix_timestamp().div_euclid(width))
            .map(|bucket| bucket.saturating_mul(width))
    });
    let upper = range.upper.as_deref().and_then(|value| {
        OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
            .ok()
            .and_then(|timestamp| {
                let seconds = timestamp.unix_timestamp();
                let bucket = seconds.div_euclid(width);
                // Midge end keys are exclusive. An exclusive predicate at a bucket boundary
                // can end at that boundary; every other upper bound needs its containing
                // bucket, followed by exact timestamp pruning.
                if range.upper_exclusive
                    && seconds.rem_euclid(width) == 0
                    && timestamp.nanosecond() == 0
                {
                    Some(bucket)
                } else {
                    bucket.checked_add(1)
                }
            })
            .map(|bucket| bucket.saturating_mul(width))
    });
    (lower, upper)
}

fn time_series_bucket_width(index: &catalog::IndexMeta) -> Option<i64> {
    let mut parts = index.options.get("bucket_width")?.split_whitespace();
    let amount = parts.next()?.parse::<i64>().ok()?;
    let unit = parts.next()?.to_ascii_lowercase();
    if amount <= 0 || parts.next().is_some() {
        return None;
    }
    let duration = match unit.as_str() {
        "minute" | "minutes" => TimeDuration::minutes(amount),
        "hour" | "hours" => TimeDuration::hours(amount),
        "day" | "days" => TimeDuration::days(amount),
        _ => return None,
    };
    Some(duration.whole_seconds())
}

fn partition_key_from_filter(
    filter: Option<&Expr>,
    partition_by: Option<&str>,
    params: &[Value],
) -> Option<String> {
    let fields = partition_by?
        .split(',')
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    if fields.is_empty() {
        return None;
    }
    let mut values = vec![None; fields.len()];
    collect_partition_equalities(filter?, &fields, &mut values, params);
    values
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .map(|values| values.join("\u{1f}"))
}

fn collect_partition_equalities(
    expr: &Expr,
    fields: &[&str],
    values: &mut [Option<String>],
    params: &[Value],
) {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => {
            collect_partition_equalities(left, fields, values, params);
            collect_partition_equalities(right, fields, values, params);
        }
        Expr::Binary {
            left,
            op: BinaryOp::Eq,
            right,
        } => {
            let (Expr::Column(column), Some(value)) =
                (left.as_ref(), partition_literal(right.as_ref(), params))
            else {
                if let (Expr::Column(column), Some(value)) =
                    (right.as_ref(), partition_literal(left.as_ref(), params))
                {
                    set_partition_value(column, value, fields, values);
                }
                return;
            };
            set_partition_value(column, value, fields, values);
        }
        _ => {}
    }
}

fn set_partition_value(
    column: &str,
    value: String,
    fields: &[&str],
    values: &mut [Option<String>],
) {
    if let Some(position) = fields
        .iter()
        .position(|field| field.eq_ignore_ascii_case(column))
    {
        values[position] = Some(value);
    }
}

fn partition_literal(expr: &Expr, params: &[Value]) -> Option<String> {
    match expr {
        Expr::StringLiteral(value) => Some(value.clone()),
        Expr::NumberLiteral(value) => Some(value.to_string()),
        Expr::BoolLiteral(value) => Some(value.to_string()),
        Expr::Null => Some("null".to_string()),
        Expr::Param(index) => value_text(params.get(*index)?),
        _ => None,
    }
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
    upper_exclusive: bool,
}

fn timestamp_range_from_filter(
    expr: &Expr,
    timestamp_field: &str,
    params: &[Value],
) -> TimestampRange {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => {
            let left = timestamp_range_from_filter(left, timestamp_field, params);
            let right = timestamp_range_from_filter(right, timestamp_field, params);
            let upper_exclusive = min_bound_exclusive(
                left.upper.as_deref(),
                left.upper_exclusive,
                right.upper.as_deref(),
                right.upper_exclusive,
            );
            TimestampRange {
                lower: max_bound(left.lower, right.lower),
                upper: min_bound(left.upper, right.upper),
                upper_exclusive,
            }
        }
        Expr::Binary { left, op, right } => {
            comparison_timestamp_range(left, op, right, timestamp_field, params).unwrap_or_default()
        }
        Expr::Between {
            expr,
            low,
            high,
            negated: false,
        } if is_timestamp_column(expr, timestamp_field) => TimestampRange {
            lower: timestamp_literal(low, params),
            upper: timestamp_literal(high, params),
            upper_exclusive: false,
        },
        _ => TimestampRange::default(),
    }
}

fn comparison_timestamp_range(
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
    timestamp_field: &str,
    params: &[Value],
) -> Option<TimestampRange> {
    if is_timestamp_column(left, timestamp_field) {
        let value = timestamp_literal(right, params)?;
        return match op {
            BinaryOp::Gt | BinaryOp::Gte => Some(TimestampRange {
                lower: Some(value),
                upper: None,
                ..TimestampRange::default()
            }),
            BinaryOp::Lt | BinaryOp::Lte => Some(TimestampRange {
                lower: None,
                upper: Some(value),
                upper_exclusive: matches!(op, BinaryOp::Lt),
            }),
            _ => None,
        };
    }
    if is_timestamp_column(right, timestamp_field) {
        let value = timestamp_literal(left, params)?;
        return match op {
            BinaryOp::Gt | BinaryOp::Gte => Some(TimestampRange {
                lower: None,
                upper: Some(value),
                upper_exclusive: matches!(op, BinaryOp::Gt),
            }),
            BinaryOp::Lt | BinaryOp::Lte => Some(TimestampRange {
                lower: Some(value),
                upper: None,
                ..TimestampRange::default()
            }),
            _ => None,
        };
    }
    None
}

fn is_timestamp_column(expr: &Expr, timestamp_field: &str) -> bool {
    matches!(expr, Expr::Column(field) if field.eq_ignore_ascii_case(timestamp_field))
}

fn timestamp_literal(expr: &Expr, params: &[Value]) -> Option<String> {
    match expr {
        Expr::StringLiteral(value) => Some(value.clone()),
        Expr::Param(index) => value_text(params.get(*index)?),
        _ => None,
    }
}

fn value_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Int64(value) => Some(value.to_string()),
        Value::Float64(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Null => Some("null".to_string()),
        Value::Json(_) | Value::Vector(_) => None,
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

fn min_bound_exclusive(
    left: Option<&str>,
    left_exclusive: bool,
    right: Option<&str>,
    right_exclusive: bool,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) if left < right => left_exclusive,
        (Some(left), Some(right)) if right < left => right_exclusive,
        (Some(_), Some(_)) => left_exclusive || right_exclusive,
        (Some(_), None) => left_exclusive,
        (None, Some(_)) => right_exclusive,
        (None, None) => false,
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
