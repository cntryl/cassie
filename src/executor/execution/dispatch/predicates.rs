//! Resolve subqueries within ternary predicates.
use super::{resolve_exists_expr, ExistsResolutionContext, Expr, ExprResolution};

pub(super) fn between<'a>(
    context: &'a ExistsResolutionContext<'a>,
    expr: &'a Expr,
    low: &'a Expr,
    high: &'a Expr,
    negated: bool,
    symmetric: bool,
) -> ExprResolution<'a> {
    Ok(Expr::Between {
        expr: Box::new(resolve_exists_expr(context, expr)?),
        low: Box::new(resolve_exists_expr(context, low)?),
        high: Box::new(resolve_exists_expr(context, high)?),
        negated,
        symmetric,
    })
}

pub(super) fn like<'a>(
    context: &'a ExistsResolutionContext<'a>,
    expr: &'a Expr,
    pattern: &'a Expr,
    escape: &'a Expr,
    case_insensitive: bool,
) -> ExprResolution<'a> {
    Ok(Expr::Like {
        expr: Box::new(resolve_exists_expr(context, expr)?),
        pattern: Box::new(resolve_exists_expr(context, pattern)?),
        escape: Box::new(resolve_exists_expr(context, escape)?),
        case_insensitive,
    })
}
