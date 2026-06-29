use super::{Cassie, CassieSession, LogicalPlan, BatchRow, QueryError, QuerySource, SelectItem, Expr, aggregate_signature, catalog, HashSet, Value};
use crate::catalog::{
    ColumnBatchFieldSummary, ColumnBatchMetadata, ColumnBatchSegmentMeta, IndexMeta,
};

pub(super) fn try_execute_column_batch_aggregate(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let QuerySource::Collection(collection) = &plan.source else {
        return Ok(None);
    };
    if session
        .is_some_and(|session| !session.collection_changes(collection).is_empty())
    {
        cassie
            .runtime
            .record_aggregate_acceleration_row_blob_fallback();
        return Ok(None);
    }
    if !eligible_plan(plan) {
        return Ok(None);
    }

    let specs = aggregate_specs(plan)?;
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
    let Some(metadata) = cassie
        .midge
        .get_column_batch_metadata(collection, &index.name)
        .map_err(|error| QueryError::General(error.to_string()))?
    else {
        cassie
            .runtime
            .record_aggregate_acceleration_row_blob_fallback();
        return Ok(None);
    };

    let mut values = Vec::with_capacity(specs.len());
    for spec in &specs {
        values.push((spec.output_name.clone(), aggregate_value(spec, &metadata)?));
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

fn aggregate_specs(plan: &LogicalPlan) -> Result<Vec<AggregateSummarySpec>, QueryError> {
    let mut specs = Vec::new();
    for item in &plan.projection {
        let SelectItem::Function { function, alias } = item else {
            return Ok(Vec::new());
        };
        let function_name = function.name.to_ascii_lowercase();
        if !matches!(
            function_name.as_str(),
            "count" | "sum" | "avg" | "min" | "max"
        ) {
            return Ok(Vec::new());
        }
        let field = match function.args.as_slice() {
            [Expr::Column(name)] if name == "*" && function_name == "count" => None,
            [Expr::Column(name)] if name != "*" => Some(name.clone()),
            _ => return Ok(Vec::new()),
        };
        specs.push(AggregateSummarySpec {
            function: function_name,
            field,
            output_name: alias
                .clone()
                .unwrap_or_else(|| aggregate_signature(function)),
        });
    }
    Ok(specs)
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
) -> Result<Value, QueryError> {
    match spec.function.as_str() {
        "count" => count_value(spec, metadata),
        "sum" => sum_value(spec, metadata),
        "avg" => avg_value(spec, metadata),
        "min" => minmax_value(spec, metadata, false),
        "max" => minmax_value(spec, metadata, true),
        _ => Ok(Value::Null),
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
            .sum::<usize>()
    } else {
        metadata
            .segments
            .iter()
            .map(|segment| segment.row_count)
            .sum()
    };
    Ok(Value::Int64(count as i64))
}

fn sum_value(
    spec: &AggregateSummarySpec,
    metadata: &ColumnBatchMetadata,
) -> Result<Value, QueryError> {
    let field = spec
        .field
        .as_ref()
        .ok_or_else(|| QueryError::General("SUM requires a field".to_string()))?;
    let mut sum = 0.0;
    let mut seen = false;
    let mut all_int = true;
    for segment in &metadata.segments {
        let Some(summary) = field_summary(segment, field) else {
            return Err(QueryError::General("missing aggregate summary".to_string()));
        };
        if let Some(value) = summary.sum {
            sum += value;
            seen = true;
            all_int &= summary.all_int;
        }
    }
    if !seen {
        return Ok(Value::Null);
    }
    if all_int {
        Ok(Value::Int64(sum as i64))
    } else {
        Ok(Value::Float64(sum))
    }
}

fn avg_value(
    spec: &AggregateSummarySpec,
    metadata: &ColumnBatchMetadata,
) -> Result<Value, QueryError> {
    let field = spec
        .field
        .as_ref()
        .ok_or_else(|| QueryError::General("AVG requires a field".to_string()))?;
    let mut sum = 0.0;
    let mut count = 0usize;
    for segment in &metadata.segments {
        let Some(summary) = field_summary(segment, field) else {
            return Err(QueryError::General("missing aggregate summary".to_string()));
        };
        if let Some(value) = summary.sum {
            sum += value;
            count += summary.non_null_count;
        }
    }
    if count == 0 {
        Ok(Value::Null)
    } else {
        Ok(Value::Float64(sum / count as f64))
    }
}

fn minmax_value(
    spec: &AggregateSummarySpec,
    metadata: &ColumnBatchMetadata,
    max: bool,
) -> Result<Value, QueryError> {
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
        let Some(candidate) = candidate.as_ref().map(json_to_value) else {
            continue;
        };
        let replace = selected
            .as_ref()
            .is_none_or(|current| compare_values(&candidate, current, max));
        if replace {
            selected = Some(candidate);
        }
    }
    Ok(selected.unwrap_or(Value::Null))
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

fn json_to_value(value: &serde_json::Value) -> Value {
    if value.is_null() {
        return Value::Null;
    }
    if let Some(value) = value.as_i64() {
        return Value::Int64(value);
    }
    if let Some(value) = value.as_f64() {
        return Value::Float64(value);
    }
    if let Some(value) = value.as_bool() {
        return Value::Bool(value);
    }
    if let Some(value) = value.as_str() {
        return Value::String(value.to_string());
    }
    Value::Json(value.clone())
}

fn compare_values(left: &Value, right: &Value, max: bool) -> bool {
    let ordering = match (left, right) {
        (Value::Int64(left), Value::Int64(right)) => left.cmp(right),
        (Value::Float64(left), Value::Float64(right)) => {
            left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)
        }
        (Value::Int64(left), Value::Float64(right)) => (*left as f64)
            .partial_cmp(right)
            .unwrap_or(std::cmp::Ordering::Equal),
        (Value::Float64(left), Value::Int64(right)) => left
            .partial_cmp(&(*right as f64))
            .unwrap_or(std::cmp::Ordering::Equal),
        (Value::String(left), Value::String(right)) => left.cmp(right),
        (Value::Bool(left), Value::Bool(right)) => left.cmp(right),
        _ => std::cmp::Ordering::Equal,
    };
    if max {
        ordering.is_gt()
    } else {
        ordering.is_lt()
    }
}
