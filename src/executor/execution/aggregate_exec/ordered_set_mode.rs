//! Mode aggregate over its single ordered-set input.

use std::cmp::Ordering;

use crate::executor::batch::BatchRow;
use crate::executor::retained_memory::value_clone_bytes;
use crate::executor::semantic::compare_values;
use crate::sql::ast::{FunctionCall, SortDirection};
use crate::types::Value;

use super::super::{check_timeout, AggregateExecutionContext, QueryError};

pub(super) fn evaluate(
    function: &FunctionCall,
    rows: &[BatchRow],
    context: &AggregateExecutionContext<'_>,
) -> Result<Value, QueryError> {
    let order = function
        .order_by
        .first()
        .ok_or_else(|| QueryError::General("mode requires WITHIN GROUP ORDER BY".into()))?;
    let mut values = Vec::new();
    let mut retained = context.controls.reserve_query_memory(0)?;
    let mut capacity = context.controls.reserve_query_memory(0)?;

    for row in rows {
        check_timeout(context.controls)?;
        if !super::super::state_input::row_filter_matches(function, row, context)? {
            continue;
        }
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
        if !matches!(value, Value::Null) {
            super::grow_values(&mut values, &mut capacity, "mode")?;
            retained.try_grow(value_clone_bytes(&value)?)?;
            values.push(value);
        }
        drop(temporary);
    }
    check_timeout(context.controls)?;
    if values.is_empty() {
        return Ok(Value::Null);
    }
    super::try_sort_by(&mut values, context.controls, |left, right| {
        let comparison = compare_values(left, right);
        match order.direction {
            SortDirection::Asc => comparison,
            SortDirection::Desc => comparison.reverse(),
        }
    })?;
    check_timeout(context.controls)?;

    let mut best = None;
    let mut best_count = 0;
    let mut index = 0;
    while index < values.len() {
        check_timeout(context.controls)?;
        let first = index;
        let value = &values[index];
        index += 1;
        while index < values.len() && compare_values(value, &values[index]) == Ordering::Equal {
            index += 1;
        }
        let count = index - first;
        if count > best_count {
            best = Some(value.clone());
            best_count = count;
        }
    }
    Ok(best.expect("a nonempty ordered-set input has a mode"))
}
