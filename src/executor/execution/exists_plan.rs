//! Classifies each `EXISTS` occurrence outside a plan's `WHERE`.
//!
//! `EXISTS` is a boolean expression that PostgreSQL accepts anywhere a boolean
//! is legal: `HAVING`, a join's `ON`, the select list and function arguments.
//! The row evaluator cannot run a subquery, so every such occurrence is
//! folded when independent or deferred to its actual row/group phase. The `WHERE`
//! filter is resolved separately by `source::resolve_plan_filter`.

use super::{ExistsResolutionContext, Expr, FunctionMeta, LogicalPlan, QueryError};
use crate::sql::ast::{QuerySource, SelectItem};

fn contains_exists(
    expr: &Expr,
    user_functions: &std::collections::HashMap<String, FunctionMeta>,
) -> bool {
    super::exists_correlated::contains_exists_or_routine_subquery(expr, user_functions)
}

fn item_exprs(item: &SelectItem) -> Vec<&Expr> {
    match item {
        SelectItem::Wildcard | SelectItem::Column { .. } => Vec::new(),
        SelectItem::Function { function, .. } => function.expressions().collect(),
        SelectItem::WindowFunction { function, .. } => function
            .args
            .iter()
            .chain(function.filter.as_deref())
            .chain(&function.partition_by)
            .chain(function.order_by.iter().map(|order| &order.expr))
            .collect(),
        SelectItem::Expr { expr, .. } => vec![expr],
    }
}

fn source_has_exists(
    source: &QuerySource,
    user_functions: &std::collections::HashMap<String, FunctionMeta>,
) -> bool {
    match source {
        QuerySource::Join {
            left, right, on, ..
        } => {
            contains_exists(on, user_functions)
                || source_has_exists(left, user_functions)
                || source_has_exists(right, user_functions)
        }
        _ => false,
    }
}

/// Returns whether `plan` has an `EXISTS` outside its `WHERE` filter.
pub(super) fn plan_has_unresolved_exists(
    plan: &LogicalPlan,
    user_functions: &std::collections::HashMap<String, FunctionMeta>,
) -> bool {
    plan.limit
        .iter()
        .chain(&plan.offset)
        .any(|expr| contains_exists(expr, user_functions))
        || plan
            .having
            .as_ref()
            .is_some_and(|expr| contains_exists(expr, user_functions))
        || plan
            .projection
            .iter()
            .flat_map(item_exprs)
            .any(|expr| contains_exists(expr, user_functions))
        || plan.projection.iter().any(|item| match item {
            SelectItem::Function { function, .. } => {
                super::filter::function_contains_routine_subquery(function, user_functions)
            }
            _ => false,
        })
        || plan
            .order
            .iter()
            .any(|order| contains_exists(&order.expr, user_functions))
        || source_has_exists(&plan.source, user_functions)
}

/// Classifies `EXISTS` outside `WHERE`, preserving reached correlated nodes.
///
/// # Errors
///
/// Returns an error when a subquery fails to bind or execute.
pub(super) fn resolve_plan_exists(
    context: &ExistsResolutionContext<'_>,
    plan: &LogicalPlan,
) -> Result<(LogicalPlan, crate::runtime::QueryMemoryReservation), QueryError> {
    let memory = super::exists_projection::reserve_clone(plan, context)?;
    let mut plan = plan.clone();
    let mut scope = None;
    if let Some(having) = &plan.having {
        plan.having = Some(super::exists_projection::classify(
            context,
            &plan.source,
            having,
            &mut scope,
        )?);
    }
    for item in &mut plan.projection {
        super::exists_projection::resolve_item(context, &plan.source, item, &mut scope)?;
    }
    for order in &mut plan.order {
        order.expr =
            super::exists_projection::classify(context, &plan.source, &order.expr, &mut scope)?;
    }
    resolve_source(context, &mut plan.source)?;
    super::subquery_bounds::fold(context, &mut plan)?;
    Ok((plan, memory))
}

fn resolve_source(
    context: &ExistsResolutionContext<'_>,
    source: &mut QuerySource,
) -> Result<(), QueryError> {
    let QuerySource::Join { on, .. } = &*source else {
        return Ok(());
    };
    let classified = super::exists_projection::classify(context, source, on, &mut None)?;
    if let QuerySource::Join {
        left, right, on, ..
    } = source
    {
        *on = classified;
        resolve_source(context, left)?;
        resolve_source(context, right)?;
    }
    Ok(())
}
