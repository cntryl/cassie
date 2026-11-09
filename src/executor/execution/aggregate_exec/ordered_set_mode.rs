//! Mode aggregate over its single ordered-set input.

#[cfg(test)]
#[path = "ordered_set_mode_output_tests.rs"]
mod output_tests;

use std::cmp::Ordering;

use crate::executor::batch::BatchRow;
use crate::executor::retained_memory::value_clone_bytes;
use crate::executor::semantic::compare_values;
use crate::sql::ast::{FunctionCall, SortDirection};
use crate::types::Value;

use super::super::{check_timeout, AggregateExecutionContext, QueryError};
use super::collection_json::{self, AggregateOutput};
use super::inputs::RetainedInputs;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ModeProgress {
    InputRetained,
    TieComparison,
    OrdinaryCandidate,
    StringCandidate,
    OutputChunk,
}

enum Best {
    StringIndex(usize),
    Ordinary(Value),
}

pub(super) fn evaluate(
    function: &FunctionCall,
    rows: &[BatchRow],
    context: &AggregateExecutionContext<'_>,
) -> Result<AggregateOutput, QueryError> {
    evaluate_owned_observed(function, rows, context, |_| {})
}

#[cfg(test)]
pub(super) fn evaluate_observed(
    function: &FunctionCall,
    rows: &[BatchRow],
    context: &AggregateExecutionContext<'_>,
    mut observe_retained: impl FnMut(),
) -> Result<Value, QueryError> {
    evaluate_owned_observed(function, rows, context, |progress| {
        if progress == ModeProgress::InputRetained {
            observe_retained();
        }
    })
    .map(AggregateOutput::into_value)
}

fn evaluate_owned_observed(
    function: &FunctionCall,
    rows: &[BatchRow],
    context: &AggregateExecutionContext<'_>,
    mut observe: impl FnMut(ModeProgress),
) -> Result<AggregateOutput, QueryError> {
    let order = function
        .order_by
        .first()
        .ok_or_else(|| QueryError::General("mode requires WITHIN GROUP ORDER BY".into()))?;
    let mut inputs = RetainedInputs {
        values: Vec::new(),
        retained: context.controls.reserve_query_memory(0)?,
        capacity: context.controls.reserve_query_memory(0)?,
    };

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
            super::grow_values(&mut inputs.values, &mut inputs.capacity, "mode")?;
            inputs.retained.try_grow(value_clone_bytes(&value)?)?;
            inputs.values.push(value);
            observe(ModeProgress::InputRetained);
            check_timeout(context.controls)?;
        }
        drop(temporary);
    }
    check_timeout(context.controls)?;
    if inputs.values.is_empty() {
        return Ok(AggregateOutput::from_value(Value::Null));
    }
    super::try_sort_by(&mut inputs.values, context.controls, |left, right| {
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
    while index < inputs.values.len() {
        check_timeout(context.controls)?;
        let first = index;
        let value = &inputs.values[index];
        index += 1;
        while index < inputs.values.len() {
            check_timeout(context.controls)?;
            let equal = compare_values(value, &inputs.values[index]) == Ordering::Equal;
            observe(ModeProgress::TieComparison);
            check_timeout(context.controls)?;
            if !equal {
                break;
            }
            index += 1;
        }
        let count = index - first;
        if count > best_count {
            best = Some(if matches!(value, Value::String(_)) {
                observe(ModeProgress::StringCandidate);
                Best::StringIndex(first)
            } else {
                let value = value.clone();
                observe(ModeProgress::OrdinaryCandidate);
                Best::Ordinary(value)
            });
            best_count = count;
        }
    }
    match best.expect("a nonempty ordered-set input has a mode") {
        Best::Ordinary(value) => Ok(AggregateOutput::from_value(value)),
        Best::StringIndex(index) => {
            let Value::String(text) = &inputs.values[index] else {
                unreachable!("a string candidate retains its sorted input")
            };
            collection_json::mode_string_output(text, &inputs.retained, context.controls, || {
                observe(ModeProgress::OutputChunk);
            })
        }
    }
}
