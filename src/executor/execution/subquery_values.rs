//! `VALUES (EXISTS (...))`, `VALUES ((SELECT ...))`: each subquery of a VALUES
//! row runs before the row is built, in the enclosing statement's CTE scope.
use super::source::SourceExecutionEnv;
use super::{
    check_timeout, resolve_exists_expr, BatchRow, CteContext, ExistsResolutionContext, Expr,
    QueryError,
};

pub(super) fn resolve(
    env: &SourceExecutionEnv<'_>,
    ctes: &CteContext,
    rows: &[Vec<Expr>],
    outer_row: Option<&BatchRow>,
) -> Result<Vec<Vec<Expr>>, QueryError> {
    let context = ExistsResolutionContext {
        cassie: env.cassie,
        session: env.session,
        cte_context: ctes,
        user_functions: env.user_functions,
        params: env.params,
        controls: env.controls,
        outer_row,
    };
    let mut resolved = Vec::new();
    resolved
        .try_reserve_exact(rows.len())
        .map_err(|error| crate::app::CassieError::ResourceLimit(error.to_string()))?;
    for row in rows {
        check_timeout(env.controls)?;
        resolved.push(
            row.iter()
                .map(|expr| resolve_exists_expr(&context, expr))
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    Ok(resolved)
}
