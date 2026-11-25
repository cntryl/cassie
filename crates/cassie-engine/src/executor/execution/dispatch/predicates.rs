//! Resolve subqueries within ternary predicates.
use super::{resolve_exists_expr, ExistsResolutionContext, Expr, ExprResolution};

pub(super) fn between(
    context: &ExistsResolutionContext<'_>,
    expr: &Expr,
    low: &Expr,
    high: &Expr,
    negated: bool,
    symmetric: bool,
) -> ExprResolution {
    Ok(Expr::Between {
        expr: Box::new(resolve_exists_expr(context, expr)?),
        low: Box::new(resolve_exists_expr(context, low)?),
        high: Box::new(resolve_exists_expr(context, high)?),
        negated,
        symmetric,
    })
}

pub(super) fn like(
    context: &ExistsResolutionContext<'_>,
    expr: &Expr,
    pattern: &Expr,
    escape: &Expr,
    case_insensitive: bool,
) -> ExprResolution {
    Ok(Expr::Like {
        expr: Box::new(resolve_exists_expr(context, expr)?),
        pattern: Box::new(resolve_exists_expr(context, pattern)?),
        escape: Box::new(resolve_exists_expr(context, escape)?),
        case_insensitive,
    })
}
