//! Offset and frame-value window functions.
use super::{
    evaluate_window_expr, frame_positions, window_arg_value, BatchRow, QueryError,
    WindowExecutionContext, WindowFrame, WindowFunctionCall,
};
use crate::types::Value;

pub(super) fn offset_window_arg_value(
    position: usize,
    indices: &[usize],
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    forward: bool,
    context: &WindowExecutionContext<'_>,
) -> Result<Value, QueryError> {
    let Some(current_index) = indices.get(position).copied() else {
        return Ok(Value::Null);
    };
    let current_row = &rows[current_index];
    let offset = if let Some(expression) = function.args.get(1) {
        let value = evaluate_window_expr(current_row, expression, context)?;
        if value.is_null() {
            return Ok(Value::Null);
        }
        let Some(offset) = value.as_i64() else {
            return Err(QueryError::General(format!(
                "{} window offset must be an integer",
                function.name
            )));
        };
        offset
    } else {
        1
    };
    let signed_position = i128::try_from(position).unwrap_or(i128::MAX);
    let signed_offset = i128::from(offset);
    let target = if forward {
        signed_position + signed_offset
    } else {
        signed_position - signed_offset
    };
    if let Some(target) = usize::try_from(target)
        .ok()
        .filter(|target| *target < indices.len())
    {
        return window_arg_value(Some(indices[target]), rows, function, context);
    }
    if let Some(default) = function.args.get(2) {
        return evaluate_window_expr(current_row, default, context);
    }
    Ok(Value::Null)
}

pub(super) fn nth_window_arg_value(
    position: usize,
    indices: &[usize],
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    frame: &WindowFrame,
    context: &WindowExecutionContext<'_>,
) -> Result<Value, QueryError> {
    let Some(current_index) = indices.get(position).copied() else {
        return Ok(Value::Null);
    };
    let Some(expression) = function.args.get(1) else {
        return Ok(Value::Null);
    };
    let value = evaluate_window_expr(&rows[current_index], expression, context)?;
    if value.is_null() {
        return Ok(Value::Null);
    }
    let Some(nth) = value.as_i64() else {
        return Err(QueryError::General(
            "nth_value position must be an integer".to_string(),
        ));
    };
    let Some(nth) = usize::try_from(nth).ok().filter(|nth| *nth > 0) else {
        return Err(QueryError::General(
            "argument of nth_value must be greater than zero".to_string(),
        ));
    };
    let positions = frame_positions(position, indices, rows, function, frame, context)?;
    let Some(target) = positions.get(nth - 1) else {
        return Ok(Value::Null);
    };
    window_arg_value(Some(indices[*target]), rows, function, context)
}
