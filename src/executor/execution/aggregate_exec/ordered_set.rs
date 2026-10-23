//! Ordered-set percentile aggregates over a bounded, retained input set.

#[path = "ordered_set_hypothetical.rs"]
mod hypothetical;
#[path = "ordered_set_mode.rs"]
mod mode;

use std::mem::size_of;

use num_traits::ToPrimitive;

use crate::app::CassieError;
use crate::executor::batch::BatchRow;
use crate::executor::filter;
use crate::executor::retained_memory::{add, value_clone_bytes};
use crate::executor::semantic::compare_values;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::sql::ast::{Expr, FunctionCall, SortDirection};
use crate::types::typed::TypedValue;
use crate::types::Value;

use super::{check_timeout, AggregateExecutionContext, QueryError};

type EvaluatedFractions = (Vec<Option<f64>>, bool, QueryMemoryReservation);

fn try_sort_by<T>(
    values: &mut [T],
    controls: &QueryExecutionControls,
    mut compare: impl FnMut(&T, &T) -> std::cmp::Ordering,
) -> Result<(), QueryError> {
    check_timeout(controls)?;
    let length = values.len();
    if length < 2 {
        return Ok(());
    }

    let sort_bytes = length
        .checked_mul(2)
        .and_then(|count| count.checked_mul(size_of::<usize>()))
        .ok_or_else(|| CassieError::ResourceLimit("ordered-set sort capacity overflow".into()))?;
    let mut sort_memory = controls.reserve_query_memory(sort_bytes)?;
    let mut indices = Vec::new();
    indices.try_reserve_exact(length).map_err(|error| {
        CassieError::ResourceLimit(format!(
            "unable to allocate ordered-set sort indices: {error}"
        ))
    })?;
    let mut scratch = Vec::new();
    scratch.try_reserve_exact(length).map_err(|error| {
        CassieError::ResourceLimit(format!(
            "unable to allocate ordered-set sort scratch: {error}"
        ))
    })?;
    let actual_capacity = indices
        .capacity()
        .checked_add(scratch.capacity())
        .and_then(|capacity| capacity.checked_mul(size_of::<usize>()))
        .ok_or_else(|| CassieError::ResourceLimit("ordered-set sort capacity overflow".into()))?;
    if actual_capacity > sort_bytes {
        sort_memory.try_grow(actual_capacity - sort_bytes)?;
    }
    indices.extend(0..length);
    scratch.resize(length, 0);

    let mut width = 1;
    while width < length {
        let mut start = 0;
        while start < length {
            let middle = start.saturating_add(width).min(length);
            let end = middle.saturating_add(width).min(length);
            let (mut left, mut right) = (start, middle);
            for output in &mut scratch[start..end] {
                check_timeout(controls)?;
                if right == end
                    || left < middle
                        && compare(&values[indices[left]], &values[indices[right]])
                            != std::cmp::Ordering::Greater
                {
                    *output = indices[left];
                    left += 1;
                } else {
                    *output = indices[right];
                    right += 1;
                }
            }
            start = end;
        }
        std::mem::swap(&mut indices, &mut scratch);
        width = width.saturating_mul(2);
    }

    for (destination, source) in indices.iter().copied().enumerate() {
        scratch[source] = destination;
    }
    for origin in 0..length {
        check_timeout(controls)?;
        while scratch[origin] != origin {
            let destination = scratch[origin];
            values.swap(origin, destination);
            scratch.swap(origin, destination);
        }
    }
    check_timeout(controls)?;
    Ok(())
}

pub(super) fn is_ordered_set(function: &FunctionCall) -> bool {
    crate::sql::functions::function(&function.name).is_some_and(|metadata| {
        matches!(
            metadata.id,
            crate::sql::functions::FunctionId::Mode
                | crate::sql::functions::FunctionId::Rank
                | crate::sql::functions::FunctionId::DenseRank
                | crate::sql::functions::FunctionId::PercentRank
                | crate::sql::functions::FunctionId::CumeDist
                | crate::sql::functions::FunctionId::PercentileCont
                | crate::sql::functions::FunctionId::PercentileDisc
        )
    })
}

pub(super) fn evaluate(
    function: &FunctionCall,
    rows: &[BatchRow],
    context: &AggregateExecutionContext<'_>,
) -> Result<Value, QueryError> {
    let metadata = crate::sql::functions::function(&function.name)
        .expect("ordered-set aggregate metadata is registered");
    match metadata.id {
        crate::sql::functions::FunctionId::Mode => mode::evaluate(function, rows, context),
        crate::sql::functions::FunctionId::Rank
        | crate::sql::functions::FunctionId::DenseRank
        | crate::sql::functions::FunctionId::PercentRank
        | crate::sql::functions::FunctionId::CumeDist => {
            hypothetical::evaluate(function, rows, context)
        }
        _ => evaluate_percentile(function, rows, context),
    }
}

fn evaluate_percentile(
    function: &FunctionCall,
    rows: &[BatchRow],
    context: &AggregateExecutionContext<'_>,
) -> Result<Value, QueryError> {
    let metadata = crate::sql::functions::function(&function.name)
        .expect("percentile aggregate metadata is registered");
    let is_continuous = metadata.id == crate::sql::functions::FunctionId::PercentileCont;
    let order = function.order_by.first().ok_or_else(|| {
        QueryError::General("percentile aggregates require WITHIN GROUP ORDER BY".into())
    })?;
    let Some((fractions, is_array, _fraction_state)) = evaluate_fractions(function, rows, context)?
    else {
        return Ok(Value::Null);
    };
    let (values, mut retained, _sort_memory) =
        collect_sorted_values(function, rows, order, context)?;

    if values.is_empty() {
        return Ok(Value::Null);
    }
    if !is_array {
        let Some(fraction) = fractions.first().copied().flatten() else {
            return Ok(Value::Null);
        };
        return percentile_value(&values, fraction, is_continuous);
    }

    percentile_array_values(&fractions, &values, &mut retained, context, is_continuous)
}

fn evaluate_fractions(
    function: &FunctionCall,
    rows: &[BatchRow],
    context: &AggregateExecutionContext<'_>,
) -> Result<Option<EvaluatedFractions>, QueryError> {
    let empty_row = BatchRow::new(Vec::new());
    let direct_row = rows.first().unwrap_or(&empty_row);
    let fraction_memory = expression_memory(
        function.args.first(),
        direct_row,
        context.params,
        context.controls,
    )?;
    let fraction = function
        .args
        .first()
        .map_or(Ok(Value::Null), |expression| {
            filter::evaluate_expr_value(
                direct_row,
                expression,
                context.params,
                context.search_context,
                context.user_functions,
                context.session,
                None,
            )
        })?;
    if matches!(fraction, Value::Null) {
        return Ok(None);
    }
    let is_array = matches!(&fraction, Value::Json(serde_json::Value::Array(_)));
    let numeric_text_array = fraction_array_has_numeric_element_type(function);
    let fractions = match fraction {
        Value::Json(serde_json::Value::Array(fractions)) => fractions
            .iter()
            .map(|value| parse_array_fraction(value, numeric_text_array))
            .collect::<Result<Vec<_>, _>>()?,
        scalar => vec![Some(parse_fraction(&scalar)?)],
    };
    let fraction_state_bytes = fractions
        .capacity()
        .checked_mul(size_of::<Option<f64>>())
        .ok_or_else(|| {
            CassieError::ResourceLimit("percentile fraction capacity overflow".into())
        })?;
    let fraction_state = context
        .controls
        .reserve_query_memory(fraction_state_bytes)?;
    drop(fraction_memory);
    Ok(Some((fractions, is_array, fraction_state)))
}

fn collect_sorted_values(
    function: &FunctionCall,
    rows: &[BatchRow],
    order: &crate::sql::ast::OrderExpr,
    context: &AggregateExecutionContext<'_>,
) -> Result<(Vec<Value>, QueryMemoryReservation, QueryMemoryReservation), QueryError> {
    let mut values = Vec::new();
    let mut retained = context.controls.reserve_query_memory(0)?;
    for row in rows {
        check_timeout(context.controls)?;
        if !super::state_input::row_filter_matches(function, row, context)? {
            continue;
        }
        let temporary =
            expression_memory(Some(&order.expr), row, context.params, context.controls)?;
        let value = filter::evaluate_expr_value(
            row,
            &order.expr,
            context.params,
            context.search_context,
            context.user_functions,
            context.session,
            None,
        )?;
        if !matches!(value, Value::Null) {
            grow_values(&mut values, &mut retained, "percentile")?;
            retained.try_grow(value_clone_bytes(&value)?)?;
            values.push(value);
        }
        drop(temporary);
    }
    check_timeout(context.controls)?;
    let sort_memory_bytes = values
        .len()
        .checked_mul(size_of::<Value>())
        .ok_or_else(|| CassieError::ResourceLimit("percentile sort capacity overflow".into()))?;
    let sort_memory = context.controls.reserve_query_memory(sort_memory_bytes)?;
    values.sort_by(|left, right| {
        let comparison = compare_values(left, right);
        match order.direction {
            SortDirection::Asc => comparison,
            SortDirection::Desc => comparison.reverse(),
        }
    });
    Ok((values, retained, sort_memory))
}

fn percentile_array_values(
    fractions: &[Option<f64>],
    values: &[Value],
    retained: &mut QueryMemoryReservation,
    context: &AggregateExecutionContext<'_>,
    is_continuous: bool,
) -> Result<Value, QueryError> {
    let json_capacity = fractions
        .len()
        .checked_mul(size_of::<serde_json::Value>())
        .ok_or_else(|| CassieError::ResourceLimit("percentile result capacity overflow".into()))?;
    retained.try_grow(json_capacity)?;
    let mut results = Vec::with_capacity(fractions.len());
    for fraction in fractions {
        check_timeout(context.controls)?;
        let result = fraction
            .map(|fraction| percentile_value(values, fraction, is_continuous))
            .transpose()?
            .unwrap_or(Value::Null);
        results.push(to_json(result));
    }
    Ok(Value::Json(serde_json::Value::Array(results)))
}

fn parse_array_fraction(
    value: &serde_json::Value,
    numeric_text: bool,
) -> Result<Option<f64>, QueryError> {
    if value.is_null() {
        return Ok(None);
    }
    let fraction = value
        .as_f64()
        .or_else(|| {
            numeric_text
                .then(|| value.as_str().and_then(|text| text.parse().ok()))
                .flatten()
        })
        .ok_or_else(|| QueryError::General("percentile fractions must be FLOAT8 values".into()))?;
    validate_fraction(fraction).map(Some)
}

fn fraction_array_has_numeric_element_type(function: &FunctionCall) -> bool {
    let Some(argument) = function.args.first() else {
        return false;
    };
    if let Expr::Subquery(subquery) = argument {
        if matches!(subquery.kind, crate::sql::ast::SubqueryKind::Array)
            && subquery
                .query
                .raw_sql
                .contains("_cassie_array_source(_cassie_array_value)")
        {
            return true;
        }
    }
    let data_type = match argument {
        Expr::Subquery(subquery)
            if matches!(subquery.kind, crate::sql::ast::SubqueryKind::Array) =>
        {
            subquery.data_type.as_ref()
        }
        Expr::Cast {
            data_type: crate::types::DataType::Array(element),
            ..
        } => Some(element.as_ref()),
        _ => None,
    };
    data_type.is_some_and(|data_type| {
        matches!(
            data_type,
            crate::types::DataType::SmallInt
                | crate::types::DataType::Int
                | crate::types::DataType::BigInt
                | crate::types::DataType::Float
                | crate::types::DataType::Extended(
                    crate::types::extended::ExtendedType::Numeric(_)
                        | crate::types::extended::ExtendedType::Real
                )
        )
    })
}

fn parse_fraction(value: &Value) -> Result<f64, QueryError> {
    let fraction = match value {
        Value::Float64(value) => *value,
        Value::Int64(value) => value.to_f64().ok_or_else(invalid_fraction_type)?,
        Value::Typed(value) => value.as_f64().ok_or_else(invalid_fraction_type)?,
        Value::String(value) => value.parse().map_err(|_| invalid_fraction_type())?,
        Value::Json(serde_json::Value::Number(value)) => {
            value.as_f64().ok_or_else(invalid_fraction_type)?
        }
        _ => return Err(invalid_fraction_type()),
    };
    validate_fraction(fraction)
}

fn validate_fraction(fraction: f64) -> Result<f64, QueryError> {
    if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
        return Err(
            CassieError::TypeInput(crate::types::extended::TypeInputError::new(
                "22003",
                format!(
                    "percentile value {} is not between 0 and 1",
                    crate::types::float_text::format(fraction)
                ),
            ))
            .into(),
        );
    }
    Ok(fraction)
}

fn invalid_fraction_type() -> QueryError {
    QueryError::General("percentile fractions must be FLOAT8 values".into())
}

fn percentile_value(
    values: &[Value],
    fraction: f64,
    continuous: bool,
) -> Result<Value, QueryError> {
    let row_count = values.len().to_f64().ok_or_else(percentile_rank_overflow)?;
    if !continuous {
        let index = (fraction * row_count)
            .ceil()
            .to_usize()
            .ok_or_else(percentile_rank_overflow)?;
        return Ok(values[index.saturating_sub(1).min(values.len() - 1)].clone());
    }
    let position = fraction
        * (values.len() - 1)
            .to_f64()
            .ok_or_else(percentile_rank_overflow)?;
    let lower = position
        .floor()
        .to_usize()
        .ok_or_else(percentile_rank_overflow)?;
    let upper = position
        .ceil()
        .to_usize()
        .ok_or_else(percentile_rank_overflow)?;
    if lower == upper {
        return continuous_value(&values[lower], &values[lower], 0.0);
    }
    let lower_position = lower.to_f64().ok_or_else(percentile_rank_overflow)?;
    continuous_value(&values[lower], &values[upper], position - lower_position)
}

fn continuous_value(lower: &Value, upper: &Value, fraction: f64) -> Result<Value, QueryError> {
    match (lower, upper) {
        (Value::Typed(TypedValue::Interval(lower)), Value::Typed(TypedValue::Interval(upper))) => {
            let difference = upper.sub(lower).map_err(interval_overflow)?;
            let scaled = difference.mul(fraction).map_err(interval_overflow)?;
            Ok(Value::Typed(TypedValue::Interval(
                lower.add(&scaled).map_err(interval_overflow)?,
            )))
        }
        (lower, upper) => {
            let lower = numeric_value(lower)?;
            let upper = numeric_value(upper)?;
            Ok(Value::Float64(lower + fraction * (upper - lower)))
        }
    }
}

fn numeric_value(value: &Value) -> Result<f64, QueryError> {
    match value {
        Value::Int64(value) => value.to_f64().ok_or_else(invalid_ordered_value),
        Value::Float64(value) => Ok(*value),
        Value::Typed(value) => value.as_f64().ok_or_else(invalid_ordered_value),
        _ => Err(invalid_ordered_value()),
    }
}

fn percentile_rank_overflow() -> QueryError {
    CassieError::ResourceLimit("percentile rank exceeds addressable range".into()).into()
}

fn invalid_ordered_value() -> QueryError {
    QueryError::General("percentile_cont requires FLOAT8 or INTERVAL ordered values".into())
}

fn interval_overflow(
    _: crate::types::extended::temporal::interval::IntervalOverflow,
) -> QueryError {
    CassieError::TypeInput(crate::types::extended::TypeInputError::new(
        "22008",
        "interval out of range".into(),
    ))
    .into()
}

fn to_json(value: Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(value) => serde_json::Value::Bool(value),
        Value::Int64(value) => serde_json::Value::from(value),
        Value::Float64(value) => serde_json::Number::from_f64(value)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        Value::String(value) => serde_json::Value::String(value),
        Value::Vector(value) => serde_json::Value::Array(
            value
                .values
                .into_iter()
                .map(serde_json::Value::from)
                .collect(),
        ),
        Value::Json(value) => value,
        Value::Typed(value) => value.to_json(),
    }
}

fn expression_memory(
    expression: Option<&Expr>,
    row: &BatchRow,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<QueryMemoryReservation, QueryError> {
    let bytes = expression.map_or(Ok(0), |expression| {
        expression_memory_bytes(expression, row, params)
    })?;
    Ok(controls.reserve_query_memory(bytes)?)
}

fn expression_memory_bytes(
    expression: &Expr,
    row: &BatchRow,
    params: &[Value],
) -> Result<usize, QueryError> {
    match expression {
        Expr::Column(name) => return Ok(row.get(name).map_or(Ok(0), value_clone_bytes)?),
        Expr::Param(index) => return Ok(params.get(*index).map_or(Ok(0), value_clone_bytes)?),
        Expr::StringLiteral(value) => return Ok(value.len()),
        Expr::Subquery(subquery)
            if matches!(subquery.kind, crate::sql::ast::SubqueryKind::Array) =>
        {
            return subquery
                .query
                .raw_sql
                .len()
                .checked_mul(16)
                .map(|bytes| bytes.max(64))
                .ok_or_else(|| {
                    CassieError::ResourceLimit("percentile fraction capacity overflow".into())
                        .into()
                });
        }
        _ => {}
    }
    let mut children = 0_usize;
    expression.try_visit_children(|child| {
        children = add(children, expression_memory_bytes(child, row, params)?)?;
        Ok::<_, QueryError>(())
    })?;
    Ok(add(children.saturating_mul(2), 64)?)
}

fn grow_values<T>(
    values: &mut Vec<T>,
    memory: &mut QueryMemoryReservation,
    kind: &str,
) -> Result<(), QueryError> {
    if values.len() == values.capacity() {
        let old = values.capacity();
        let next = old.max(1).checked_mul(2).ok_or_else(|| {
            QueryError::from(CassieError::ResourceLimit(format!(
                "{kind} input capacity overflow"
            )))
        })?;
        memory.try_grow(next.checked_mul(size_of::<T>()).ok_or_else(|| {
            CassieError::ResourceLimit(format!("{kind} input capacity overflow"))
        })?)?;
        values
            .try_reserve_exact(next - values.len())
            .map_err(|error| {
                CassieError::ResourceLimit(format!("unable to allocate {kind} inputs: {error}"))
            })?;
        let actual = values.capacity();
        if actual > next {
            memory.try_grow((actual - next).checked_mul(size_of::<T>()).ok_or_else(|| {
                CassieError::ResourceLimit(format!("{kind} input capacity overflow"))
            })?)?;
        }
        memory.shrink_to(
            memory
                .bytes()
                .saturating_sub(old.saturating_mul(size_of::<T>())),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use crate::config::CassieRuntimeLimits;
    use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

    use super::try_sort_by;

    #[test]
    fn should_order_values_when_an_ordered_set_sort_completes() {
        // Arrange
        let limits = CassieRuntimeLimits::default();
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let mut values = vec![7, 1, 4, 1, 9, 2];

        // Act
        try_sort_by(&mut values, &controls, i32::cmp).expect("sort completes");

        // Assert
        assert_eq!(values, [1, 1, 2, 4, 7, 9]);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_cancel_ordered_set_sort_after_comparisons_begin() {
        // Arrange
        let limits = CassieRuntimeLimits::default();
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &limits,
            Instant::now(),
            cancellation.clone(),
        );
        let mut values = (0..64).rev().collect::<Vec<_>>();
        let mut comparisons = 0;

        // Act
        let error = try_sort_by(&mut values, &controls, |left, right| {
            comparisons += 1;
            if comparisons == 3 {
                cancellation.cancel();
            }
            left.cmp(right)
        })
        .expect_err("sort observes cancellation after comparisons begin");

        // Assert
        assert!(error.to_string().contains("query canceled"));
        assert!(comparisons >= 3);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
