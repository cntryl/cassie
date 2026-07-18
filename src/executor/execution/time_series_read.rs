use super::{
    batch, catalog, check_timeout, ensure_query_memory_budget, filter, projection, sort, BatchRow,
    BinaryOp, Cassie, CassieSession, CollectionSchema, DataType, Expr, FunctionMeta, HashMap,
    LogicalPlan, QueryError, QueryExecutionControls, QuerySource, RowDecode, Value,
};
use crate::midge::adapter::time_series_indexes::{
    ControlledTimeSeriesDocumentScanOutcome, ControlledTimeSeriesIndexScanOutcome,
};
use crate::midge::adapter::DocumentRef;
use crate::runtime::QueryMemoryReservation;
use time::{Duration as TimeDuration, OffsetDateTime};

use super::projected_read::json_to_query_value;

#[path = "time_series_read/memory.rs"]
mod memory;

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
        session,
        collection: &spec.collection,
        scan_fields: &scan_fields,
        index: &index,
        range: &range,
        partition_key: partition_key.as_deref(),
        lower_bucket_seconds,
        upper_bucket_seconds,
    };
    let selected = time_series_documents(cassie, &scan_request, controls)?;
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
    execute_time_series_rows(selected, &context)
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
    selected: TimeSeriesDocuments,
    context: &TimeSeriesExecutionContext<'_>,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let TimeSeriesDocuments {
        mut documents,
        index_entries_scanned,
        row_point_fetches,
        indexed_total_buckets,
        bucket_native,
        fallback_reason,
        memory: _source_memory,
    } = selected;
    check_timeout(context.controls)?;
    sort_time_series_documents(&mut documents, context.timestamp_field);
    let total_buckets = indexed_total_buckets.unwrap_or_else(|| {
        count_document_buckets(&documents, context.timestamp_field, context.index)
    });
    let accounted = memory::document_batches(
        documents,
        context.scan_fields,
        context.schema,
        context.controls,
    )?;
    let batches = accounted.batches;
    let batch_memory = accounted.memory;
    let before_filter_buckets =
        count_batch_row_buckets(&batches, context.timestamp_field, context.index);
    let (mut batches, mut batch_memory) =
        filter_time_series_batches(batches, batch_memory, context)?;
    let (scanned_buckets, skipped_buckets) = time_series_bucket_metrics(
        &batches,
        context.timestamp_field,
        context.index,
        total_buckets,
        before_filter_buckets,
    );
    if !context.plan.order.is_empty() {
        let eval = sort::EvalInput {
            order: &context.plan.order,
            projection: &context.plan.projection,
            params: context.params,
            search_context: None,
            user_functions: context.user_functions,
            session: context.session,
        };
        let cloned_input_memory = ensure_query_memory_budget(context.controls, &batches)?;
        let sorted_batches =
            sort::sort_batches_with_controls(batches.clone(), &eval, context.controls)?;
        let replacement_memory = ensure_query_memory_budget(context.controls, &sorted_batches)?;
        drop(cloned_input_memory);
        drop(batch_memory);
        batches = sorted_batches;
        batch_memory = replacement_memory;
    }
    let cloned_input_memory = ensure_query_memory_budget(context.controls, &batches)?;
    let projected_batches = projection::project_batches(
        batches.clone(),
        &context.plan.projection,
        context.params,
        None,
        context.user_functions,
        context.session,
    )?;
    let replacement_memory = ensure_query_memory_budget(context.controls, &projected_batches)?;
    drop(cloned_input_memory);
    drop(batch_memory);
    batches = projected_batches;
    batch_memory = replacement_memory;
    if let Some((offset, limit)) = batch_window(context.plan) {
        let moved_input_memory = ensure_query_memory_budget(context.controls, &batches)?;
        let sliced_batches = batch::slice_batches(batches, offset, limit);
        let replacement_memory = ensure_query_memory_budget(context.controls, &sliced_batches)?;
        drop(moved_input_memory);
        drop(batch_memory);
        batches = sliced_batches;
        batch_memory = replacement_memory;
    }
    finish_time_series_rows(
        batches,
        batch_memory,
        context,
        &TimeSeriesReadMetrics {
            scanned_buckets,
            skipped_buckets,
            index_entries_scanned,
            row_point_fetches,
            bucket_native,
            fallback_reason,
        },
    )
}

struct TimeSeriesReadMetrics {
    scanned_buckets: usize,
    skipped_buckets: usize,
    index_entries_scanned: usize,
    row_point_fetches: usize,
    bucket_native: bool,
    fallback_reason: Option<&'static str>,
}

fn finish_time_series_rows(
    batches: Vec<batch::Batch>,
    batch_memory: QueryMemoryReservation,
    context: &TimeSeriesExecutionContext<'_>,
    metrics: &TimeSeriesReadMetrics,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let (rows, _flatten_memory) = memory::finalize_batches(batches, context.controls)?;
    drop(batch_memory);
    if metrics.bucket_native {
        context
            .cassie
            .runtime
            .record_time_series_bucket_native_hit();
    }
    if let Some(reason) = metrics.fallback_reason {
        context.cassie.runtime.record_time_series_fallback(reason);
    }
    context.cassie.runtime.record_time_series_scan(
        &context.index.name,
        rows.len(),
        metrics.scanned_buckets,
        metrics.skipped_buckets,
        metrics.index_entries_scanned,
        metrics.row_point_fetches,
    );
    Ok(Some(rows))
}

fn sort_time_series_documents(documents: &mut [DocumentRef], timestamp_field: &str) {
    documents.sort_unstable_by(|left, right| {
        timestamp_sort_key(&left.payload, timestamp_field)
            .cmp(timestamp_sort_key(&right.payload, timestamp_field))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn filter_time_series_batches(
    batches: Vec<batch::Batch>,
    batch_memory: QueryMemoryReservation,
    context: &TimeSeriesExecutionContext<'_>,
) -> Result<(Vec<batch::Batch>, QueryMemoryReservation), QueryError> {
    let Some(filter_expr) = &context.plan.filter else {
        return Ok((batches, batch_memory));
    };
    let cloned_input_memory = ensure_query_memory_budget(context.controls, &batches)?;
    let filtered_batches = filter::filter_batches(
        batches.clone(),
        filter_expr,
        context.params,
        None,
        context.user_functions,
        context.session,
    )?;
    let replacement_memory = ensure_query_memory_budget(context.controls, &filtered_batches)?;
    drop(cloned_input_memory);
    drop(batch_memory);
    Ok((filtered_batches, replacement_memory))
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
    batches: &[batch::Batch],
    timestamp_field: &str,
    index: &catalog::IndexMeta,
    total_buckets: usize,
    before_filter_buckets: usize,
) -> (usize, usize) {
    let scanned_buckets = count_batch_row_buckets(batches, timestamp_field, index);
    let skipped_buckets = total_buckets
        .max(before_filter_buckets)
        .saturating_sub(scanned_buckets);
    (scanned_buckets, skipped_buckets)
}

struct TimeSeriesScanRequest<'a> {
    session: Option<&'a CassieSession>,
    collection: &'a str,
    scan_fields: &'a [String],
    index: &'a catalog::IndexMeta,
    range: &'a TimestampRange,
    partition_key: Option<&'a str>,
    lower_bucket_seconds: Option<i64>,
    upper_bucket_seconds: Option<i64>,
}

struct TimeSeriesDocuments {
    documents: Vec<DocumentRef>,
    index_entries_scanned: usize,
    row_point_fetches: usize,
    indexed_total_buckets: Option<usize>,
    bucket_native: bool,
    fallback_reason: Option<&'static str>,
    memory: Vec<QueryMemoryReservation>,
}

fn time_series_documents(
    cassie: &Cassie,
    request: &TimeSeriesScanRequest<'_>,
    controls: &QueryExecutionControls,
) -> Result<TimeSeriesDocuments, QueryError> {
    match cassie.midge.scan_time_series_index_controlled(
        request.index,
        request.partition_key,
        request.lower_bucket_seconds,
        request.upper_bucket_seconds,
        controls,
    ) {
        Ok(ControlledTimeSeriesIndexScanOutcome::Native(report)) => {
            let indexed_total_buckets = Some(hit_bucket_count(report.hits.as_slice()));
            let hits = prune_hits_for_range(report.hits, request.range);
            if hits.is_empty() {
                return Ok(TimeSeriesDocuments {
                    documents: Vec::new(),
                    index_entries_scanned: report.entries_scanned,
                    row_point_fetches: 0,
                    indexed_total_buckets,
                    bucket_native: true,
                    fallback_reason: None,
                    memory: report.memory,
                });
            }
            let documents = cassie
                .midge
                .scan_time_series_hit_documents_controlled(
                    request.index,
                    &hits,
                    request.scan_fields,
                    controls,
                )
                .map_err(QueryError::from)?;
            match documents {
                ControlledTimeSeriesDocumentScanOutcome::Native(documents)
                    if cassie
                        .midge
                        .collection_generation(request.collection)
                        .map_err(QueryError::from)?
                        == report.generation =>
                {
                    let mut memory = report.memory;
                    memory.push(documents.memory);
                    Ok(TimeSeriesDocuments {
                        documents: documents.documents,
                        index_entries_scanned: report.entries_scanned,
                        row_point_fetches: hits.len(),
                        indexed_total_buckets,
                        bucket_native: true,
                        fallback_reason: None,
                        memory,
                    })
                }
                ControlledTimeSeriesDocumentScanOutcome::Native(_) => scan_row_backed_documents(
                    cassie,
                    request.collection,
                    request.scan_fields,
                    request.session,
                    controls,
                    report.entries_scanned,
                    "stale-bucket-metadata",
                ),
                ControlledTimeSeriesDocumentScanOutcome::Fallback(reason) => {
                    scan_row_backed_documents(
                        cassie,
                        request.collection,
                        request.scan_fields,
                        request.session,
                        controls,
                        report.entries_scanned,
                        reason,
                    )
                }
            }
        }
        Ok(ControlledTimeSeriesIndexScanOutcome::Fallback(reason)) => scan_row_backed_documents(
            cassie,
            request.collection,
            request.scan_fields,
            request.session,
            controls,
            0,
            reason,
        ),
        Err(error) => time_series_error_fallback(cassie, request, controls, error),
    }
}

fn time_series_error_fallback(
    cassie: &Cassie,
    request: &TimeSeriesScanRequest<'_>,
    controls: &QueryExecutionControls,
    error: crate::app::CassieError,
) -> Result<TimeSeriesDocuments, QueryError> {
    if matches!(
        error,
        crate::app::CassieError::QueryCancelled
            | crate::app::CassieError::DeadlineExceeded
            | crate::app::CassieError::ResourceLimit(_)
    ) {
        return Err(QueryError::from(error));
    }
    let reason = if matches!(error, crate::app::CassieError::Unsupported(_)) {
        "unsupported-bucket-width"
    } else {
        "corrupt-bucket-metadata"
    };
    scan_row_backed_documents(
        cassie,
        request.collection,
        request.scan_fields,
        request.session,
        controls,
        0,
        reason,
    )
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

fn hit_bucket_count(
    hits: &[crate::midge::adapter::time_series_indexes::TimeSeriesIndexScanHit],
) -> usize {
    hits.iter()
        .map(|hit| hit.bucket_key.as_str())
        .fold((None, 0usize), |(previous, count), bucket| {
            if previous == Some(bucket) {
                (previous, count)
            } else {
                (Some(bucket), count.saturating_add(1))
            }
        })
        .1
}

fn scan_row_backed_documents(
    cassie: &Cassie,
    collection: &str,
    scan_fields: &[String],
    session: Option<&CassieSession>,
    controls: &QueryExecutionControls,
    index_entries_scanned: usize,
    fallback_reason: &'static str,
) -> Result<TimeSeriesDocuments, QueryError> {
    let Some(mut cursor) = cassie
        .open_session_row_cursor(
            session,
            collection,
            RowDecode::ProjectedHistorical(scan_fields.to_vec()),
            controls,
        )
        .map_err(QueryError::from)?
    else {
        return Err(QueryError::General(
            "time-series row fallback requires row storage".to_string(),
        ));
    };
    let mut documents = Vec::new();
    let mut memory = Vec::new();
    loop {
        let accounted = cursor
            .next_accounted_documents(&cassie.midge, batch::DEFAULT_BATCH_SIZE, controls)
            .map_err(QueryError::from)?;
        if accounted.is_empty() {
            break;
        }
        for document in accounted {
            let (document, reservation) = document.into_parts();
            documents.push(document);
            memory.push(reservation);
        }
    }
    Ok(TimeSeriesDocuments {
        documents,
        index_entries_scanned,
        row_point_fetches: 0,
        indexed_total_buckets: None,
        bucket_native: false,
        fallback_reason: Some(fallback_reason),
        memory,
    })
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

fn timestamp_sort_key<'a>(payload: &'a serde_json::Value, field: &str) -> &'a str {
    payload_field(payload, field)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
}

fn count_document_buckets(
    documents: &[DocumentRef],
    timestamp_field: &str,
    index: &catalog::IndexMeta,
) -> usize {
    count_ordered_buckets(
        documents.iter().filter_map(|document| {
            payload_field(&document.payload, timestamp_field).and_then(serde_json::Value::as_str)
        }),
        bucket_precision(index),
    )
}

fn count_batch_row_buckets(
    batches: &[batch::Batch],
    timestamp_field: &str,
    index: &catalog::IndexMeta,
) -> usize {
    count_ordered_buckets(
        batches.iter().flatten().filter_map(|row| {
            row.get(timestamp_field).and_then(|value| match value {
                Value::String(value) => Some(value.as_str()),
                _ => None,
            })
        }),
        bucket_precision(index),
    )
}

#[derive(Clone, Copy)]
enum BucketPrecision {
    Hour,
    Day,
    Exact,
}

fn bucket_precision(index: &catalog::IndexMeta) -> BucketPrecision {
    let Some(width) = index.options.get("bucket_width") else {
        return BucketPrecision::Exact;
    };
    if width
        .split_whitespace()
        .any(|part| part.eq_ignore_ascii_case("hour") || part.eq_ignore_ascii_case("hours"))
    {
        BucketPrecision::Hour
    } else if width
        .split_whitespace()
        .any(|part| part.eq_ignore_ascii_case("day") || part.eq_ignore_ascii_case("days"))
    {
        BucketPrecision::Day
    } else {
        BucketPrecision::Exact
    }
}

fn count_ordered_buckets<'a>(
    timestamps: impl Iterator<Item = &'a str>,
    precision: BucketPrecision,
) -> usize {
    timestamps
        .map(|timestamp| match precision {
            BucketPrecision::Hour => timestamp.get(..13).unwrap_or(timestamp),
            BucketPrecision::Day => timestamp.get(..10).unwrap_or(timestamp),
            BucketPrecision::Exact => timestamp,
        })
        .fold((None, 0usize), |(previous, count), bucket| {
            if previous == Some(bucket) {
                (previous, count)
            } else {
                (Some(bucket), count.saturating_add(1))
            }
        })
        .1
}
