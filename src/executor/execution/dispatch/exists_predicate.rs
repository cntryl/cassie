//! Execute a resolved EXISTS predicate with the enclosing statement context.
use super::{build_exists_logical_plan, execute_plan_with_outer_row, plan_execution_env};
use super::{ExistsResolutionContext, Expr, QueryError};

pub(super) fn execute<'a>(
    context: &'a ExistsResolutionContext<'a>,
    statement: &'a crate::sql::ast::ParsedStatement,
) -> Result<Expr, QueryError> {
    let logical = build_exists_logical_plan(context, statement)?;
    let (logical, _bound_memory) =
        if crate::sql::pagination::plan_needs_resolution(&logical, context.controls)? {
            let (resolved, reservation) =
                crate::sql::pagination::resolve_plan(&logical, context.params, context.controls)?;
            (resolved, Some(reservation))
        } else {
            (logical, None)
        };
    let logical = super::super::exists_target::bounded_exists_logical(logical);
    let mut subquery_context = context.cte_context.copy(context.controls)?;
    let env = plan_execution_env(
        context.cassie,
        context.session,
        context.user_functions,
        context.params,
        context.controls,
    );
    let outer_row = context
        .outer_row
        .map(|row| super::super::exists_correlated::scoped_outer_row(context, &logical, row))
        .transpose()?;
    let rows =
        execute_plan_with_outer_row(&env, &logical, &mut subquery_context, outer_row.as_ref())?;
    Ok(Expr::BoolLiteral(!rows.is_empty()))
}
