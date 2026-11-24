//! Shared scalar CASE evaluation retains branch order and lazy results.
use super::{
    eq_value, eval_scalar_with_context, EvalContext, Expr, QueryError, RowAccess, ScalarValue,
};
pub(super) fn evaluate<R: RowAccess + ?Sized>(
    row: &R,
    operand: Option<&Expr>,
    branches: &[(Expr, Expr)],
    else_expr: Option<&Expr>,
    context: EvalContext<'_>,
) -> Result<ScalarValue, QueryError> {
    let operand_value = operand
        .map(|operand| eval_scalar_with_context(row, operand, context))
        .transpose()?;
    for (when, then) in branches {
        let when_value = eval_scalar_with_context(row, when, context)?;
        if operand_value.is_none()
            && !matches!(when_value, ScalarValue::Bool(_) | ScalarValue::Null)
        {
            return Err(QueryError::General(
                "CASE WHEN condition must be BOOLEAN".to_string(),
            ));
        }
        let selected = if let Some(operand_value) = &operand_value {
            eq_value(operand_value, &when_value) == Some(true)
        } else {
            matches!(when_value, ScalarValue::Bool(true))
        };
        if selected {
            return eval_scalar_with_context(row, then, context);
        }
    }
    else_expr.map_or(Ok(ScalarValue::Null), |expr| {
        eval_scalar_with_context(row, expr, context)
    })
}
