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

#[cfg(test)]
#[path = "subquery_eval_tests.rs"]
mod tests;

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
    retain_rows_observing(rows, subquery, context.controls, |_, _| {})
}

fn retain_rows_observing(
    rows: Vec<super::BatchRow>,
    subquery: &SubqueryExpr,
    controls: &crate::runtime::QueryExecutionControls,
    mut observe_move: impl FnMut(usize, usize),
) -> Result<SubqueryRows, QueryError> {
    use crate::executor::retained_memory::{add, mul, value_clone_bytes};
    use crate::types::Value;
    use std::mem::size_of;

    let width = subquery.required_width();
    let single_row = matches!(
        subquery.kind,
        SubqueryKind::Scalar | SubqueryKind::Row { .. }
    );
    if single_row && rows.len() > 1 {
        return Err(crate::app::CassieError::CardinalityViolation(MULTIPLE_ROWS.into()).into());
    }
    // The rows stay attached to the expression for every outer row that
    // evaluates it. Admit both directories and the Arc header as well as the
    // existing retained-value estimate before consuming their source owners.
    let mut bytes = add(size_of::<SubqueryRows>(), 2 * size_of::<usize>())?;
    bytes = add(bytes, mul(rows.len(), size_of::<Vec<Value>>())?)?;
    for row in &rows {
        bytes = add(bytes, mul(row.entries().len(), size_of::<Value>())?)?;
        for (_, value) in row.entries() {
            bytes = add(bytes, value_clone_bytes(value)?)?;
        }
    }
    // Declare the lease first so every partially retained directory drops first
    // on any error, including width and cancellation failures.
    let memory = controls.reserve_query_memory(bytes)?;
    let mut values = Vec::new();
    reserve_exact(&mut values, rows.len())?;
    for (row_index, row) in rows.into_iter().enumerate() {
        super::check_timeout(controls)?;
        let _query_memory = row.query_memory();
        let _operator_memory = row.operator_memory();
        let count = row.entries().len();
        let mut fields = Vec::new();
        reserve_exact(&mut fields, count)?;
        for (field_index, (_, value)) in row.into_entries().into_iter().enumerate() {
            fields.push(value);
            observe_move(row_index, field_index);
            super::check_timeout(controls)?;
        }
        if fields.len() != width {
            return Err(subquery.width_error(fields.len()).into());
        }
        values.push(fields);
    }
    if single_row && values.len() > controls.max_result_rows {
        return Err(crate::app::CassieError::ResourceLimit(format!(
            "query result row limit exceeded: {} > {}",
            values.len(),
            controls.max_result_rows
        ))
        .into());
    }
    Ok(SubqueryRows {
        rows: values,
        memory: Some(memory),
    })
}

fn reserve_exact<T>(values: &mut Vec<T>, count: usize) -> Result<(), QueryError> {
    values
        .try_reserve_exact(count)
        .map_err(|error| crate::app::CassieError::ResourceLimit(error.to_string()))?;
    if values.capacity() > count {
        return Err(crate::app::CassieError::ResourceLimit(
            "subquery directory allocation exceeded its admission".into(),
        )
        .into());
    }
    Ok(())
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
        QueryError::FloatPowerOverflow | QueryError::FloatPowerUnderflow => false,
        QueryError::General(message) => {
            message.eq_ignore_ascii_case("query canceled")
                || message.eq_ignore_ascii_case("query timeout exceeded")
        }
        QueryError::Cassie(error) => matches!(
            error,
            crate::app::CassieError::QueryCanceled | crate::app::CassieError::DeadlineExceeded
        ),
    }
}
