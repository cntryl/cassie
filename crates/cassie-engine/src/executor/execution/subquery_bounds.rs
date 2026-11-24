//! `LIMIT (SELECT ...)` and `OFFSET (SELECT ...)`: PostgreSQL evaluates the
//! bound once, before reading the source, as an integer.
use super::{filter, resolve_exists_expr, ExistsResolutionContext, Expr, LogicalPlan, QueryError};
use crate::types::Value;

pub(super) fn fold(
    context: &ExistsResolutionContext<'_>,
    plan: &mut LogicalPlan,
) -> Result<(), QueryError> {
    plan.limit = bound(context, plan.limit.take(), "LIMIT")?;
    plan.offset = bound(context, plan.offset.take(), "OFFSET")?;
    Ok(())
}

fn bound(
    context: &ExistsResolutionContext<'_>,
    bound: Option<Expr>,
    clause: &str,
) -> Result<Option<Expr>, QueryError> {
    let Some(expr) = bound else {
        return Ok(None);
    };
    if !expr.contains_subquery() {
        return Ok(Some(expr));
    }
    let resolved = resolve_exists_expr(context, &expr)?;
    let value = filter::evaluate_expr_value(
        &Vec::<(String, Value)>::new(),
        &resolved,
        context.params,
        None,
        context.user_functions,
        context.session,
        None,
    )?;
    let value = match value {
        Value::Null => return Ok(None),
        Value::Int64(value) => value,
        _ => return Err(type_error(clause)),
    };
    if value < 0 {
        return Err(crate::app::CassieError::Diagnostic(Box::new(
            crate::app::DiagnosticError::new(
                if clause == "LIMIT" { "2201W" } else { "2201X" },
                format!("{clause} must not be negative"),
            ),
        ))
        .into());
    }
    Ok(Some(Expr::IntegerLiteral(value)))
}

fn type_error(clause: &str) -> QueryError {
    crate::app::CassieError::Diagnostic(Box::new(crate::app::DiagnosticError::new(
        "42804",
        format!("argument of {clause} must be type bigint"),
    )))
    .into()
}
