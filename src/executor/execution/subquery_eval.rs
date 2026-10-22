//! Runs a scalar, array, row or quantified subquery and attaches its rows.
//!
//! Binding errors propagate, so a correlated subquery that cannot bind
//! without its outer row is retried per row by the caller. Execution errors
//! are kept in the outcome and raised only when the evaluator reaches the
//! node, which preserves PostgreSQL's lazy CASE/COALESCE behavior.
//!
//! A scalar or row subquery is bounded to two rows: the second row is only
//! needed to report the cardinality violation, so no larger result is ever
//! materialized to find it.
use std::sync::Arc;

use super::dispatch::build_exists_logical_plan;
use super::{
    execute_plan_with_outer_row, plan_execution_env, resolve_exists_expr, ExistsResolutionContext,
    Expr, LogicalPlan, QueryError,
};
use crate::sql::ast::{SubqueryExpr, SubqueryKind, SubqueryOutcome, SubqueryRows};

pub(crate) use crate::sql::ast::SUBQUERY_MULTIPLE_ROWS as MULTIPLE_ROWS;

pub(super) fn resolve(
    context: &ExistsResolutionContext<'_>,
    subquery: &SubqueryExpr,
) -> Result<Expr, QueryError> {
    let mut resolved = subquery.clone();
    for operand in resolved.operands_mut() {
        *operand = resolve_exists_expr(context, operand)?;
    }
    if resolved.outcome.is_none() {
        resolved.outcome = Some(execute(context, subquery)?);
    }
    Ok(Expr::Subquery(Box::new(resolved)))
}

fn execute(
    context: &ExistsResolutionContext<'_>,
    subquery: &SubqueryExpr,
) -> Result<SubqueryOutcome, QueryError> {
    let single_row = matches!(
        subquery.kind,
        SubqueryKind::Scalar | SubqueryKind::Row { .. }
    );
    let controls = if single_row {
        context.controls.for_uncapped_input()
    } else {
        context.controls.clone()
    };
    let logical = build_exists_logical_plan(context, &subquery.query)?;
    let (logical, _bound_memory) =
        if crate::sql::pagination::plan_needs_resolution(&logical, &controls)? {
            let (resolved, reservation) =
                crate::sql::pagination::resolve_plan(&logical, context.params, &controls)?;
            (resolved, Some(reservation))
        } else {
            (logical, None)
        };
    let logical = bounded(logical, &subquery.kind);
    let outer_row = context
        .outer_row
        .map(|row| super::exists_correlated::scoped_outer_row(context, &logical, row))
        .transpose()?;
    match run(context, &logical, outer_row.as_ref(), subquery, &controls) {
        Ok(rows) => Ok(SubqueryOutcome::Rows(Arc::new(rows))),
        Err(error) if is_interruption(&error) => Err(error),
        Err(error) => Ok(SubqueryOutcome::failed(error.into())),
    }
}

fn run(
    context: &ExistsResolutionContext<'_>,
    logical: &LogicalPlan,
    outer_row: Option<&super::BatchRow>,
    subquery: &SubqueryExpr,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<SubqueryRows, QueryError> {
    let mut subquery_context = context.cte_context.copy(controls)?;
    let env = plan_execution_env(
        context.cassie,
        context.session,
        context.user_functions,
        context.params,
        controls,
    );
    let rows = execute_plan_with_outer_row(&env, logical, &mut subquery_context, outer_row)?;
    let width = subquery.required_width();
    let single_row = matches!(
        subquery.kind,
        SubqueryKind::Scalar | SubqueryKind::Row { .. }
    );
    if single_row && rows.len() > 1 {
        return Err(crate::app::CassieError::CardinalityViolation(MULTIPLE_ROWS.into()).into());
    }
    let mut values = Vec::new();
    values
        .try_reserve_exact(rows.len())
        .map_err(|error| crate::app::CassieError::ResourceLimit(error.to_string()))?;
    // The rows stay attached to the expression for every outer row that
    // evaluates it, so they are admitted like any other retained result.
    let mut bytes = std::mem::size_of::<SubqueryRows>();
    for row in &rows {
        for (_, value) in row.entries() {
            bytes = crate::executor::retained_memory::add(
                bytes,
                crate::executor::retained_memory::value_clone_bytes(value)?,
            )?;
        }
    }
    let memory = context.controls.reserve_query_memory(bytes)?;
    for row in rows {
        super::check_timeout(context.controls)?;
        let fields = row
            .into_entries()
            .into_iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>();
        if fields.len() != width {
            return Err(subquery.width_error(fields.len()).into());
        }
        values.push(fields);
    }
    if single_row && values.len() > context.controls.max_result_rows {
        return Err(crate::app::CassieError::ResourceLimit(format!(
            "query result row limit exceeded: {} > {}",
            values.len(),
            context.controls.max_result_rows
        ))
        .into());
    }
    Ok(SubqueryRows {
        rows: values,
        memory: Some(memory),
    })
}

/// Keeps at most two rows of a scalar or row subquery.
fn bounded(mut logical: LogicalPlan, kind: &SubqueryKind) -> LogicalPlan {
    if !matches!(kind, SubqueryKind::Scalar | SubqueryKind::Row { .. }) {
        return logical;
    }
    let limit = match (&logical.limit, logical.limit_value()) {
        (None, _) => 2,
        (Some(_), Some(limit)) => limit.min(2),
        (Some(_), None) => return logical,
    };
    logical.limit = Some(Expr::IntegerLiteral(limit));
    logical
}

pub(super) fn is_interruption(error: &QueryError) -> bool {
    match error {
        QueryError::General(message) => {
            message.eq_ignore_ascii_case("query canceled")
                || message.eq_ignore_ascii_case("query timeout exceeded")
        }
        QueryError::Cassie(error) => matches!(
            error,
            crate::app::CassieError::QueryCancelled | crate::app::CassieError::DeadlineExceeded
        ),
    }
}
