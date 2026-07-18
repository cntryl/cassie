use super::{
    aggregate_signature, catalog, BatchRow, Cassie, CassieSession, Expr, HashSet, LogicalPlan,
    QueryError, QueryExecutionControls, QuerySource, SelectItem, Value,
};
use crate::catalog::{
    ColumnBatchFieldSummary, ColumnBatchMetadata, ColumnBatchNumericSum, ColumnBatchSegmentMeta,
    IndexMeta,
};
use crate::midge::adapter::ControlledColumnBatchSummaryDecision;
use crate::types::semantic::compare_values;

pub(super) fn try_execute_column_batch_aggregate(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let QuerySource::Collection(collection) = &plan.source else {
        return Ok(None);
    };
    if session.is_some_and(|session| !session.collection_changes(collection).is_empty()) {
        cassie
            .runtime
            .record_aggregate_acceleration_row_blob_fallback();
        return Ok(None);
    }
    if !eligible_plan(plan) {
        return Ok(None);
    }

    let specs = aggregate_specs(plan);
    let fields = specs
        .iter()
        .filter_map(|spec| spec.field.as_ref())
        .map(|field| field.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let Some(index) = covering_column_index(cassie, collection, fields.as_slice()) else {
        cassie
            .runtime
            .record_aggregate_acceleration_row_blob_fallback();
        return Ok(None);
    };
    let controlled = match cassie
        .midge
        .prepare_column_batch_summaries_controlled(collection, &index, fields.as_slice(), controls)
        .map_err(QueryError::from)?
    {
        ControlledColumnBatchSummaryDecision::Ready(controlled) => controlled,
        ControlledColumnBatchSummaryDecision::Fallback(reason) => {
            cassie.runtime.record_column_batch_fallback(reason.as_str());
            cassie
                .runtime
                .record_aggregate_acceleration_row_blob_fallback();
            return Ok(None);
        }
    };
    let metadata = *controlled.metadata;
    let _summary_memory = controlled.memory;

    let mut values = Vec::with_capacity(specs.len());
    for spec in &specs {
        let value = match aggregate_value(spec, &metadata)? {
            AggregateSummaryValue::Ready(value) => value,
            AggregateSummaryValue::Fallback(reason) => {
                cassie.runtime.record_column_batch_fallback(reason);
                cassie
                    .runtime
                    .record_aggregate_acceleration_row_blob_fallback();
                return Ok(None);
            }
        };
        values.push((spec.output_name.clone(), value));
    }
    if cassie
        .midge
        .collection_generation(collection)
        .map_err(|error| QueryError::General(error.to_string()))?
        != metadata.built_generation
    {
        cassie
            .runtime
            .record_column_batch_fallback("generation_mismatch");
        cassie
            .runtime
            .record_aggregate_acceleration_row_blob_fallback();
        return Ok(None);
    }
    cassie
        .runtime
        .record_aggregate_acceleration(metadata.segments.len());
    Ok(Some(vec![BatchRow::new(values)]))
}

#[derive(Debug)]
struct AggregateSummarySpec {
    function: String,
    field: Option<String>,
    output_name: String,
}

enum AggregateSummaryValue {
    Ready(Value),
    Fallback(&'static str),
}

fn eligible_plan(plan: &LogicalPlan) -> bool {
    plan.command.is_none()
        && plan.ctes.is_empty()
        && plan.filter.is_none()
        && plan.group_by.is_empty()
        && plan.having.is_none()
        && plan.order.is_empty()
        && plan.limit.is_none()
        && plan.offset.unwrap_or(0) == 0
        && !plan.distinct
        && plan.distinct_on.is_empty()
        && plan.set.is_none()
}

fn aggregate_specs(plan: &LogicalPlan) -> Vec<AggregateSummarySpec> {
    let mut specs = Vec::new();
    for item in &plan.projection {
        let SelectItem::Function { function, alias } = item else {
            return Vec::new();
        };
        let function_name = function.name.to_ascii_lowercase();
        if !matches!(
            function_name.as_str(),
            "count" | "sum" | "avg" | "min" | "max"
        ) {
            return Vec::new();
        }
        let field = match function.args.as_slice() {
            [Expr::Column(name)] if name == "*" && function_name == "count" => None,
            [Expr::Column(name)] if name != "*" => Some(name.clone()),
            _ => return Vec::new(),
        };
        specs.push(AggregateSummarySpec {
            function: function_name,
            field,
            output_name: alias
                .clone()
                .unwrap_or_else(|| aggregate_signature(function)),
        });
    }
    specs
}

fn covering_column_index(
    cassie: &Cassie,
    collection: &str,
    fields: &[String],
) -> Option<IndexMeta> {
    cassie
        .catalog
        .list_indexes(collection)
        .into_iter()
        .filter(|index| index.kind == catalog::IndexKind::Column)
        .find(|index| {
            let available = index
                .normalized_fields()
                .into_iter()
                .map(|field| field.to_ascii_lowercase())
                .collect::<HashSet<_>>();
            fields.iter().all(|field| available.contains(field)) || fields.is_empty()
        })
}

fn aggregate_value(
    spec: &AggregateSummarySpec,
    metadata: &ColumnBatchMetadata,
) -> Result<AggregateSummaryValue, QueryError> {
    match spec.function.as_str() {
        "count" => count_value(spec, metadata).map(AggregateSummaryValue::Ready),
        "sum" => sum_value(spec, metadata).map(|value| {
            value.map_or(
                AggregateSummaryValue::Fallback("numeric_summary_requires_rows"),
                AggregateSummaryValue::Ready,
            )
        }),
        "avg" => avg_value(spec, metadata).map(|value| {
            value.map_or(
                AggregateSummaryValue::Fallback("numeric_summary_requires_rows"),
                AggregateSummaryValue::Ready,
            )
        }),
        "min" => minmax_value(spec, metadata, false).map(|value| {
            value.map_or(
                AggregateSummaryValue::Fallback("typed_summary_requires_rows"),
                AggregateSummaryValue::Ready,
            )
        }),
        "max" => minmax_value(spec, metadata, true).map(|value| {
            value.map_or(
                AggregateSummaryValue::Fallback("typed_summary_requires_rows"),
                AggregateSummaryValue::Ready,
            )
        }),
        _ => Ok(AggregateSummaryValue::Ready(Value::Null)),
    }
}

fn count_value(
    spec: &AggregateSummarySpec,
    metadata: &ColumnBatchMetadata,
) -> Result<Value, QueryError> {
    let count = if let Some(field) = &spec.field {
        metadata
            .segments
            .iter()
            .map(|segment| field_summary(segment, field).map(|summary| summary.non_null_count))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| QueryError::General("missing aggregate summary".to_string()))?
            .into_iter()
            .try_fold(0usize, usize::checked_add)
            .ok_or_else(|| QueryError::General("aggregate row count overflow".to_string()))?
    } else {
        metadata
            .segments
            .iter()
            .map(|segment| segment.row_count)
            .try_fold(0usize, usize::checked_add)
            .ok_or_else(|| QueryError::General("aggregate row count overflow".to_string()))?
    };
    i64::try_from(count)
        .map(Value::Int64)
        .map_err(|_| QueryError::General("aggregate row count overflow".to_string()))
}

fn sum_value(
    spec: &AggregateSummarySpec,
    metadata: &ColumnBatchMetadata,
) -> Result<Option<Value>, QueryError> {
    let field = spec
        .field
        .as_ref()
        .ok_or_else(|| QueryError::General("SUM requires a field".to_string()))?;
    let summaries = field_summaries(metadata, field)?;
    if summaries.iter().all(|summary| summary.numeric_count == 0) {
        return Ok(Some(Value::Null));
    }
    if let Some(sum) = fold_checked_integer_summaries(&summaries)? {
        return i64::try_from(sum)
            .map(Value::Int64)
            .map(Some)
            .map_err(|_| QueryError::General("aggregate integer overflow".to_string()));
    }
    if !floating_summary_fold_is_exact(&summaries) {
        return Ok(None);
    }
    merge_float_capable_summaries(&summaries).map(Some)
}

fn merge_numeric_sums(
    current: ColumnBatchNumericSum,
    next: &ColumnBatchNumericSum,
) -> Result<ColumnBatchNumericSum, QueryError> {
    use ColumnBatchNumericSum::{Empty, Float, FloatEmpty, Integer, IntegerOverflow};
    match (current, next.clone()) {
        (IntegerOverflow, _) | (_, IntegerOverflow) => Err(QueryError::General(
            "aggregate integer overflow".to_string(),
        )),
        (Empty, next) => Ok(next.clone()),
        (current, Empty) => Ok(current),
        (FloatEmpty, FloatEmpty) => Ok(FloatEmpty),
        (FloatEmpty, Integer(value)) | (Integer(value), FloatEmpty) => Ok(Float(i64_to_f64(value))),
        (FloatEmpty, Float(value)) | (Float(value), FloatEmpty) => Ok(Float(value)),
        (Integer(left), Integer(right)) => left
            .checked_add(right)
            .map(Integer)
            .ok_or_else(|| QueryError::General("aggregate integer overflow".to_string())),
        (Integer(left), Float(right)) => Ok(Float(i64_to_f64(left) + right)),
        (Float(left), Integer(right)) => Ok(Float(left + i64_to_f64(right))),
        (Float(left), Float(right)) => Ok(Float(left + right)),
    }
}

fn avg_value(
    spec: &AggregateSummarySpec,
    metadata: &ColumnBatchMetadata,
) -> Result<Option<Value>, QueryError> {
    let field = spec
        .field
        .as_ref()
        .ok_or_else(|| QueryError::General("AVG requires a field".to_string()))?;
    let summaries = field_summaries(metadata, field)?;
    let mut count = 0usize;
    for summary in &summaries {
        count = count
            .checked_add(summary.numeric_count)
            .ok_or_else(|| QueryError::General("aggregate row count overflow".to_string()))?;
    }
    if count == 0 {
        return Ok(Some(Value::Null));
    }
    if let Some(sum) = fold_exact_f64_integer_summaries(&summaries) {
        return Ok(Some(Value::Float64(i128_to_f64(sum) / usize_to_f64(count))));
    }
    if !floating_summary_fold_is_exact(&summaries) {
        return Ok(None);
    }
    let sum: f64 = summaries.iter().filter_map(|summary| summary.avg_sum).sum();
    Ok(Some(Value::Float64(sum / usize_to_f64(count))))
}

fn minmax_value(
    spec: &AggregateSummarySpec,
    metadata: &ColumnBatchMetadata,
    max: bool,
) -> Result<Option<Value>, QueryError> {
    let field = spec
        .field
        .as_ref()
        .ok_or_else(|| QueryError::General("MIN/MAX requires a field".to_string()))?;
    let mut selected: Option<Value> = None;
    for segment in &metadata.segments {
        let Some(summary) = field_summary(segment, field) else {
            return Err(QueryError::General("missing aggregate summary".to_string()));
        };
        let candidate = if max { &summary.max } else { &summary.min };
        let Some(candidate) = candidate.as_ref() else {
            continue;
        };
        if matches!(candidate, Value::Vector(_) | Value::Json(_)) {
            return Ok(None);
        }
        let replace = selected.as_ref().is_none_or(|current| {
            let ordering = compare_values(candidate, current);
            if max {
                ordering.is_gt()
            } else {
                ordering.is_lt()
            }
        });
        if replace {
            selected = Some(candidate.clone());
        }
    }
    Ok(Some(selected.unwrap_or(Value::Null)))
}

fn field_summary<'a>(
    segment: &'a ColumnBatchSegmentMeta,
    field: &str,
) -> Option<&'a ColumnBatchFieldSummary> {
    segment
        .summaries
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(field))
        .map(|(_, summary)| summary)
}

fn field_summaries<'a>(
    metadata: &'a ColumnBatchMetadata,
    field: &str,
) -> Result<Vec<&'a ColumnBatchFieldSummary>, QueryError> {
    metadata
        .segments
        .iter()
        .map(|segment| {
            field_summary(segment, field)
                .ok_or_else(|| QueryError::General("missing aggregate summary".to_string()))
        })
        .collect()
}

fn fold_checked_integer_summaries(
    summaries: &[&ColumnBatchFieldSummary],
) -> Result<Option<i128>, QueryError> {
    let mut incoming = 0_i128;
    for summary in summaries {
        if summary.numeric_count == 0 {
            if summary.non_null_count > 0 {
                return Ok(None);
            }
            continue;
        }
        if summary.non_null_count != summary.numeric_count {
            return Ok(None);
        }
        let (Some(total), Some(prefix_min), Some(prefix_max)) = (
            summary.integer_total,
            summary.integer_prefix_min,
            summary.integer_prefix_max,
        ) else {
            return Ok(None);
        };
        let minimum = incoming.checked_add(prefix_min);
        let maximum = incoming.checked_add(prefix_max);
        if minimum.is_none_or(|value| value < i128::from(i64::MIN))
            || maximum.is_none_or(|value| value > i128::from(i64::MAX))
        {
            return Err(QueryError::General(
                "aggregate integer overflow".to_string(),
            ));
        }
        incoming = incoming
            .checked_add(total)
            .ok_or_else(|| QueryError::General("aggregate integer overflow".to_string()))?;
    }
    Ok(Some(incoming))
}

fn fold_exact_f64_integer_summaries(summaries: &[&ColumnBatchFieldSummary]) -> Option<i128> {
    const MAX_EXACT_F64_INTEGER: i128 = 1_i128 << 53;
    let mut incoming = 0_i128;
    for summary in summaries {
        if summary.numeric_count == 0 {
            continue;
        }
        let (Some(total), Some(prefix_min), Some(prefix_max)) = (
            summary.integer_total,
            summary.integer_prefix_min,
            summary.integer_prefix_max,
        ) else {
            return None;
        };
        let minimum = incoming.checked_add(prefix_min)?;
        let maximum = incoming.checked_add(prefix_max)?;
        if minimum < -MAX_EXACT_F64_INTEGER || maximum > MAX_EXACT_F64_INTEGER {
            return None;
        }
        incoming = incoming.checked_add(total)?;
    }
    Some(incoming)
}

fn floating_summary_fold_is_exact(summaries: &[&ColumnBatchFieldSummary]) -> bool {
    summaries.len() <= 1
        || summaries.iter().all(|summary| {
            summary.numeric_count <= 1 && summary.non_null_count == summary.numeric_count
        })
}

fn merge_float_capable_summaries(
    summaries: &[&ColumnBatchFieldSummary],
) -> Result<Value, QueryError> {
    let mut sum = ColumnBatchNumericSum::Empty;
    for summary in summaries {
        sum = merge_numeric_sums(sum, &summary.sum)?;
    }
    match sum {
        ColumnBatchNumericSum::Empty | ColumnBatchNumericSum::FloatEmpty => Ok(Value::Null),
        ColumnBatchNumericSum::Integer(sum) => Ok(Value::Int64(sum)),
        ColumnBatchNumericSum::Float(sum) => Ok(Value::Float64(sum)),
        ColumnBatchNumericSum::IntegerOverflow => Err(QueryError::General(
            "aggregate integer overflow".to_string(),
        )),
    }
}

fn usize_to_f64(value: usize) -> f64 {
    value.to_string().parse::<f64>().unwrap_or(f64::INFINITY)
}

fn i64_to_f64(value: i64) -> f64 {
    value
        .to_string()
        .parse::<f64>()
        .expect("i64 should convert to finite f64")
}

fn i128_to_f64(value: i128) -> f64 {
    value
        .to_string()
        .parse::<f64>()
        .expect("bounded i128 should convert to finite f64")
}
