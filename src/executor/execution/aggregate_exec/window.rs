//! Controlled frame folds share the existing grouped accumulator math.
use std::mem::size_of;

use crate::executor::batch::BatchRow;
use crate::executor::filter::EvaluationMemory;
use crate::executor::retained_memory::{add, value_clone_bytes};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{Expr, FunctionCall, WindowFunctionCall};
use crate::types::Value;

use super::state::AggregateAccumulator;
use super::QueryError;

pub(super) fn evaluate<'a>(
    function: &WindowFunctionCall,
    rows: impl IntoIterator<Item = &'a BatchRow>,
    params: &[Value],
    controls: &QueryExecutionControls,
    output: &EvaluationMemory,
    evaluate: impl Fn(&BatchRow, &Expr, &EvaluationMemory) -> Result<Value, QueryError>,
) -> Result<Value, QueryError> {
    controls.check_interruption()?;
    let _call_memory =
        controls.reserve_query_memory(add(size_of::<FunctionCall>(), function.name.len())?)?;
    let name = match crate::sql::functions::aggregate_window_function(&function.name)
        .map(|metadata| metadata.id)
    {
        Some(crate::sql::functions::FunctionId::Count) => "count",
        Some(crate::sql::functions::FunctionId::Sum) => "sum",
        _ => function.name.as_str(),
    };
    let aggregate = FunctionCall {
        name: name.to_owned(),
        ..FunctionCall::default()
    };
    let mut accumulator = AggregateAccumulator::new(&aggregate);
    let mut retained = controls.reserve_query_memory(accumulator.retained_bytes())?;
    for row in rows {
        controls.check_interruption()?;
        let inputs = EvaluationMemory::new(controls)?;
        let value = |expression: &Expr| {
            let _scratch = controls.reserve_query_memory(
                super::super::source::source_set_function::argument_scratch(
                    expression, params, row,
                )?,
            )?;
            let value = evaluate(row, expression, &inputs)?;
            inputs.grow(value_clone_bytes(&value)?)?;
            Ok::<_, QueryError>(value)
        };
        if let Some(predicate) = &function.filter {
            match value(predicate)? {
                Value::Bool(true) => {}
                Value::Bool(false) | Value::Null => continue,
                _ => {
                    return Err(QueryError::General(
                        "FILTER requires BOOLEAN or SQL NULL".into(),
                    ))
                }
            }
        }
        let first = match function.args.first() {
            Some(expression) => value(expression)?,
            None => Value::Null,
        };
        let second = match function.args.get(1) {
            Some(expression) => Some(value(expression)?),
            None => None,
        };
        let _working = controls.reserve_query_memory(working_bytes(&accumulator, Some(&first))?)?;
        #[cfg(test)]
        tests::before_transition(controls);
        accumulator.update_values(&aggregate, first, None, second)?;
        let bytes = accumulator.retained_bytes();
        if bytes > retained.bytes() {
            retained.try_grow(bytes - retained.bytes())?;
        } else {
            retained.shrink_to(bytes);
        }
        controls.aggregate_input_retained()?;
        #[cfg(test)]
        tests::after_transition();
        controls.check_interruption()?;
    }
    controls.check_interruption()?;
    let _working = controls.reserve_query_memory(working_bytes(&accumulator, None)?)?;
    let result = accumulator.finish()?;
    // Result allocation remains covered by finalization workspace until its
    // physical ownership is transferred into the shared window-row lease.
    output.grow(value_clone_bytes(&result)?)?;
    controls.check_interruption()?;
    Ok(result)
}

fn working_bytes(
    accumulator: &AggregateAccumulator,
    input: Option<&Value>,
) -> Result<usize, QueryError> {
    match accumulator {
        AggregateAccumulator::Statistical(state) => state.working_bytes(input),
        _ => Ok(0),
    }
}

#[cfg(test)]
#[path = "window_tests.rs"]
mod tests;
