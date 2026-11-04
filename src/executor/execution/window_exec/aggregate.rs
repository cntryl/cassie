//! Registered aggregate windows reuse the controlled frame transition boundary.
use super::{BatchRow, QueryError, Value, WindowExecutionContext, WindowFunctionCall};
use crate::executor::filter::{evaluation::ProjectionValueContext, EvaluationMemory};
use crate::sql::ast::{Expr, FunctionCall};
use crate::sql::functions::{aggregate_window_function, FunctionId};
use crate::types::DataType;

pub(super) fn selected(function: &WindowFunctionCall) -> bool {
    aggregate_window_function(&function.name).is_some()
}

pub(super) fn data_type(
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    context: &WindowExecutionContext<'_>,
) -> DataType {
    let metadata = aggregate_window_function(&function.name).expect("registered window aggregate");
    if crate::sql::functions::regression::is_regression_function(metadata.id) {
        return if metadata.id == FunctionId::RegrCount {
            DataType::BigInt
        } else {
            DataType::Float
        };
    }
    if crate::sql::functions::statistical::is_statistical_function(metadata.id) {
        let input = rows
            .first()
            .zip(function.args.first())
            .and_then(|(row, argument)| {
                crate::executor::array_order::expression_type(row, argument, context.user_functions)
            });
        return match input {
            Some(
                DataType::Float | DataType::Extended(crate::types::extended::ExtendedType::Real),
            ) => DataType::Float,
            _ => DataType::Extended(crate::types::extended::ExtendedType::Numeric(None)),
        };
    }
    rows.first()
        .and_then(|row| {
            let aggregate = FunctionCall {
                name: function.name.clone(),
                args: function.args.clone(),
                ..FunctionCall::default()
            };
            crate::executor::array_order::expression_type(
                row,
                &Expr::Function(Box::new(aggregate)),
                context.user_functions,
            )
        })
        .unwrap_or(match metadata.id {
            FunctionId::BoolAnd | FunctionId::BoolOr => DataType::Boolean,
            _ => DataType::BigInt,
        })
}

pub(super) fn evaluate<'a>(
    rows: impl IntoIterator<Item = &'a BatchRow>,
    function: &WindowFunctionCall,
    context: &WindowExecutionContext<'_>,
) -> Result<Value, QueryError> {
    let output = context
        .output_memory
        .expect("scalar window owns output admission");
    super::super::aggregate_exec::evaluate_window_aggregate(
        function,
        rows,
        context.params,
        context.controls,
        output,
        |row, expression, memory| evaluate_argument(row, expression, memory, context),
    )
}

fn evaluate_argument(
    row: &BatchRow,
    expression: &Expr,
    memory: &EvaluationMemory,
    context: &WindowExecutionContext<'_>,
) -> Result<Value, QueryError> {
    if let Some(evaluate) = context.evaluate {
        return evaluate(row, expression);
    }
    let projection = ProjectionValueContext {
        params: context.params,
        search: context.search_context,
        functions: context.user_functions,
        session: context.session,
        resolver: None,
    };
    crate::executor::filter::evaluation::evaluate_projection_value(
        row,
        expression,
        &projection,
        context.controls,
        memory,
    )
}

pub(super) fn metadata_bytes(rows: &[BatchRow], data_type: &DataType) -> Result<usize, QueryError> {
    use crate::executor::retained_memory::{add, data_type_clone_bytes, mul};
    let Some(row) = rows.first() else { return Ok(0); };
    if row.data_types().is_empty() && !matches!(data_type, DataType::Array(_)) { return Ok(0); }
    let slots = row.entries().len().max(row.data_types().len()).checked_add(1).ok_or_else(|| crate::app::CassieError::ResourceLimit("window type metadata overflow".into()))?;
    let mut bytes = add(mul(slots, std::mem::size_of::<DataType>())?, std::mem::size_of::<Vec<DataType>>() + 2 * std::mem::size_of::<usize>())?;
    for kind in row.data_types().iter().chain(std::iter::once(data_type)) { bytes = add(bytes, data_type_clone_bytes(kind)?)?; }
    Ok(bytes)
}
