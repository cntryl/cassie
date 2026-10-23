//! Hypothetical-set rank aggregates over ordered input tuples.

use std::cmp::Ordering;
use std::mem::size_of;

use num_traits::ToPrimitive;

use crate::app::CassieError;
use crate::executor::batch::BatchRow;
use crate::executor::retained_memory::value_clone_bytes;
use crate::executor::semantic::compare_values;
use crate::runtime::QueryMemoryReservation;
use crate::sql::ast::{FunctionCall, NullsOrder, OrderExpr, SortDirection};
use crate::sql::functions::FunctionId;
use crate::types::Value;

use super::super::{check_timeout, AggregateExecutionContext, QueryError};

struct DirectInputs {
    arguments: Vec<Value>,
    _capacity: QueryMemoryReservation,
    retained: QueryMemoryReservation,
}

struct OrderedInputs {
    rows: Vec<Vec<Value>>,
    _capacity: QueryMemoryReservation,
    _key_capacity: QueryMemoryReservation,
}

#[derive(Clone, Copy)]
struct RankCounts {
    less: usize,
    less_or_equal: usize,
    dense_less: usize,
}

pub(super) fn evaluate(
    function: &FunctionCall,
    rows: &[BatchRow],
    context: &AggregateExecutionContext<'_>,
) -> Result<Value, QueryError> {
    let order_by = &function.order_by;
    if order_by.is_empty() || function.args.len() != order_by.len() {
        return Err(QueryError::General(
            "hypothetical-set aggregates require matching direct arguments and WITHIN GROUP ORDER BY keys".into(),
        ));
    }
    let mut direct = collect_direct_inputs(function, rows, context)?;
    let inputs = collect_ordered_inputs(function, rows, context, &mut direct.retained)?;
    let counts = rank_counts(&inputs.rows, &direct.arguments, order_by, context)?;
    rank_result(function, inputs.rows.len(), counts)
}

fn collect_direct_inputs(
    function: &FunctionCall,
    rows: &[BatchRow],
    context: &AggregateExecutionContext<'_>,
) -> Result<DirectInputs, QueryError> {
    let empty_row = BatchRow::new(Vec::new());
    let direct_row = rows.first().unwrap_or(&empty_row);
    let direct_capacity_bytes = function
        .args
        .len()
        .checked_mul(size_of::<Value>())
        .ok_or_else(|| {
            CassieError::ResourceLimit("hypothetical-set argument capacity overflow".into())
        })?;
    let mut direct_capacity = context
        .controls
        .reserve_query_memory(direct_capacity_bytes)?;
    let mut direct = Vec::new();
    direct
        .try_reserve_exact(function.args.len())
        .map_err(|error| {
            CassieError::ResourceLimit(format!(
                "unable to allocate hypothetical-set arguments: {error}"
            ))
        })?;
    let actual_direct_capacity = direct.capacity();
    if actual_direct_capacity > function.args.len() {
        direct_capacity.try_grow(
            (actual_direct_capacity - function.args.len())
                .checked_mul(size_of::<Value>())
                .ok_or_else(|| {
                    CassieError::ResourceLimit("hypothetical-set argument capacity overflow".into())
                })?,
        )?;
    }
    let mut retained = context.controls.reserve_query_memory(0)?;
    for expression in &function.args {
        let temporary = super::expression_memory(
            Some(expression),
            direct_row,
            context.params,
            context.controls,
        )?;
        let value = crate::executor::filter::evaluate_expr_value(
            direct_row,
            expression,
            context.params,
            context.search_context,
            context.user_functions,
            context.session,
            None,
        )?;
        retained.try_grow(value_clone_bytes(&value)?)?;
        direct.push(value);
        drop(temporary);
    }
    Ok(DirectInputs {
        arguments: direct,
        _capacity: direct_capacity,
        retained,
    })
}

fn collect_ordered_inputs(
    function: &FunctionCall,
    rows: &[BatchRow],
    context: &AggregateExecutionContext<'_>,
    retained: &mut QueryMemoryReservation,
) -> Result<OrderedInputs, QueryError> {
    let order_by = &function.order_by;
    let mut inputs = Vec::new();
    let mut capacity = context.controls.reserve_query_memory(0)?;
    let mut key_storage = context.controls.reserve_query_memory(0)?;
    for row in rows {
        check_timeout(context.controls)?;
        if !super::super::state_input::row_filter_matches(function, row, context)? {
            continue;
        }
        let key_capacity_bytes =
            order_by
                .len()
                .checked_mul(size_of::<Value>())
                .ok_or_else(|| {
                    CassieError::ResourceLimit("hypothetical-set key capacity overflow".into())
                })?;
        key_storage.try_grow(key_capacity_bytes)?;
        let mut key = Vec::new();
        key.try_reserve_exact(order_by.len()).map_err(|error| {
            CassieError::ResourceLimit(format!("unable to allocate hypothetical-set key: {error}"))
        })?;
        let actual_key_capacity = key.capacity();
        if actual_key_capacity > order_by.len() {
            key_storage.try_grow(
                (actual_key_capacity - order_by.len())
                    .checked_mul(size_of::<Value>())
                    .ok_or_else(|| {
                        CassieError::ResourceLimit("hypothetical-set key capacity overflow".into())
                    })?,
            )?;
        }
        for order in order_by {
            check_timeout(context.controls)?;
            let temporary =
                super::expression_memory(Some(&order.expr), row, context.params, context.controls)?;
            let value = crate::executor::filter::evaluate_expr_value(
                row,
                &order.expr,
                context.params,
                context.search_context,
                context.user_functions,
                context.session,
                None,
            )?;
            retained.try_grow(value_clone_bytes(&value)?)?;
            key.push(value);
            drop(temporary);
        }
        super::grow_values(&mut inputs, &mut capacity, "hypothetical-set row")?;
        inputs.push(key);
    }
    check_timeout(context.controls)?;
    super::try_sort_by(&mut inputs, context.controls, |left, right| {
        compare_rows(left, right, order_by)
    })?;
    check_timeout(context.controls)?;
    Ok(OrderedInputs {
        rows: inputs,
        _capacity: capacity,
        _key_capacity: key_storage,
    })
}

fn rank_counts(
    inputs: &[Vec<Value>],
    direct: &[Value],
    order_by: &[OrderExpr],
    context: &AggregateExecutionContext<'_>,
) -> Result<RankCounts, QueryError> {
    let mut less = 0_usize;
    let mut less_or_equal = 0_usize;
    let mut dense_less = 0_usize;
    let mut previous_lower: Option<&[Value]> = None;
    for input in inputs {
        check_timeout(context.controls)?;
        match compare_rows(input, direct, order_by) {
            Ordering::Less => {
                less = checked_increment(less, "hypothetical rank")?;
                if previous_lower.is_none_or(|previous| {
                    compare_rows(previous, input, order_by) != Ordering::Equal
                }) {
                    dense_less = checked_increment(dense_less, "hypothetical dense rank")?;
                }
                previous_lower = Some(input);
                less_or_equal = checked_increment(less_or_equal, "hypothetical cume_dist")?;
            }
            Ordering::Equal => {
                less_or_equal = checked_increment(less_or_equal, "hypothetical cume_dist")?;
            }
            Ordering::Greater => {}
        }
    }
    Ok(RankCounts {
        less,
        less_or_equal,
        dense_less,
    })
}

fn rank_result(
    function: &FunctionCall,
    input_count: usize,
    counts: RankCounts,
) -> Result<Value, QueryError> {
    match crate::sql::functions::function(&function.name)
        .expect("hypothetical-set metadata is registered")
        .id
    {
        FunctionId::Rank => Ok(Value::Int64(rank_value(counts.less)?)),
        FunctionId::DenseRank => Ok(Value::Int64(rank_value(counts.dense_less)?)),
        FunctionId::PercentRank => {
            let value = if input_count == 0 {
                0.0
            } else {
                ratio(counts.less, input_count)?
            };
            Ok(Value::Float64(value))
        }
        FunctionId::CumeDist => {
            let denominator = checked_increment(input_count, "hypothetical cume_dist")?;
            let numerator = checked_increment(counts.less_or_equal, "hypothetical cume_dist")?;
            Ok(Value::Float64(ratio(numerator, denominator)?))
        }
        _ => unreachable!("only hypothetical-set aggregates reach this evaluator"),
    }
}

fn compare_rows(left: &[Value], right: &[Value], order_by: &[OrderExpr]) -> Ordering {
    for ((left, right), order) in left.iter().zip(right).zip(order_by) {
        let comparison = compare_ordered_values(left, right, order);
        if comparison != Ordering::Equal {
            return comparison;
        }
    }
    left.len().cmp(&right.len())
}

fn compare_ordered_values(left: &Value, right: &Value, order: &OrderExpr) -> Ordering {
    match (left, right) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => {
            if nulls_first(order) {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
        (_, Value::Null) => {
            if nulls_first(order) {
                Ordering::Greater
            } else {
                Ordering::Less
            }
        }
        _ => {
            let comparison = compare_values(left, right);
            match order.direction {
                SortDirection::Asc => comparison,
                SortDirection::Desc => comparison.reverse(),
            }
        }
    }
}

fn nulls_first(order: &OrderExpr) -> bool {
    match order.nulls {
        Some(NullsOrder::First) => true,
        Some(NullsOrder::Last) => false,
        None => matches!(order.direction, SortDirection::Desc),
    }
}

fn checked_increment(value: usize, description: &str) -> Result<usize, QueryError> {
    value
        .checked_add(1)
        .ok_or_else(|| CassieError::ResourceLimit(format!("{description} overflow")).into())
}

fn rank_value(preceding_rows: usize) -> Result<i64, QueryError> {
    i64::try_from(checked_increment(preceding_rows, "hypothetical rank")?)
        .map_err(|_| CassieError::ResourceLimit("hypothetical rank exceeds BIGINT".into()).into())
}

fn ratio(numerator: usize, denominator: usize) -> Result<f64, QueryError> {
    let numerator = numerator
        .to_f64()
        .ok_or_else(|| CassieError::ResourceLimit("hypothetical rank exceeds FLOAT8".into()))?;
    let denominator = denominator
        .to_f64()
        .filter(|value| *value != 0.0)
        .ok_or_else(|| CassieError::ResourceLimit("hypothetical rank exceeds FLOAT8".into()))?;
    Ok(numerator / denominator)
}
