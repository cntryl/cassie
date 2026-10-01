//! Resolves uncorrelated `EXISTS` subqueries outside a plan's `WHERE`.
//!
//! `EXISTS` is a boolean expression that PostgreSQL accepts anywhere a boolean
//! is legal: `HAVING`, a join's `ON`, the select list and function arguments.
//! The row evaluator cannot run a subquery, so every such occurrence is
//! replaced by its boolean result before the plan executes. The `WHERE`
//! filter is resolved separately by `source::resolve_plan_filter`.

use super::{resolve_exists_expr, ExistsResolutionContext, Expr, LogicalPlan, QueryError};
use crate::sql::ast::{OrderExpr, QuerySource, SelectItem};

fn contains_exists(expr: &Expr) -> bool {
    expr.any_descendant_or_self(&mut |expr| matches!(expr, Expr::Exists(_)))
}

fn item_exprs(item: &SelectItem) -> Vec<&Expr> {
    match item {
        SelectItem::Wildcard | SelectItem::Column { .. } => Vec::new(),
        SelectItem::Function { function, .. } => function.args.iter().collect(),
        SelectItem::WindowFunction { function, .. } => function
            .args
            .iter()
            .chain(&function.partition_by)
            .chain(function.order_by.iter().map(|order| &order.expr))
            .collect(),
        SelectItem::Expr { expr, .. } => vec![expr],
    }
}

fn source_has_exists(source: &QuerySource) -> bool {
    match source {
        QuerySource::Join {
            left, right, on, ..
        } => contains_exists(on) || source_has_exists(left) || source_has_exists(right),
        _ => false,
    }
}

/// Returns whether `plan` has an `EXISTS` outside its `WHERE` filter.
pub(super) fn plan_has_unresolved_exists(plan: &LogicalPlan) -> bool {
    plan.having.as_ref().is_some_and(contains_exists)
        || plan
            .projection
            .iter()
            .flat_map(item_exprs)
            .any(contains_exists)
        || plan.order.iter().any(|order| contains_exists(&order.expr))
        || source_has_exists(&plan.source)
}

/// Returns `plan` with every `EXISTS` outside `WHERE` replaced by its result.
///
/// # Errors
///
/// Returns an error when a subquery fails to bind or execute.
pub(super) fn resolve_plan_exists(
    context: &ExistsResolutionContext<'_>,
    plan: &LogicalPlan,
) -> Result<LogicalPlan, QueryError> {
    let mut plan = plan.clone();
    if let Some(having) = &plan.having {
        plan.having = Some(resolve_exists_expr(context, having)?);
    }
    for item in &mut plan.projection {
        resolve_item(context, item)?;
    }
    for order in &mut plan.order {
        resolve_order(context, order)?;
    }
    resolve_source(context, &mut plan.source)?;
    Ok(plan)
}

fn resolve_all(
    context: &ExistsResolutionContext<'_>,
    exprs: &mut [Expr],
) -> Result<(), QueryError> {
    for expr in exprs {
        *expr = resolve_exists_expr(context, expr)?;
    }
    Ok(())
}

fn resolve_order(
    context: &ExistsResolutionContext<'_>,
    order: &mut OrderExpr,
) -> Result<(), QueryError> {
    order.expr = resolve_exists_expr(context, &order.expr)?;
    Ok(())
}

fn resolve_item(
    context: &ExistsResolutionContext<'_>,
    item: &mut SelectItem,
) -> Result<(), QueryError> {
    match item {
        SelectItem::Wildcard | SelectItem::Column { .. } => Ok(()),
        SelectItem::Function { function, .. } => resolve_all(context, &mut function.args),
        SelectItem::WindowFunction { function, .. } => {
            resolve_all(context, &mut function.args)?;
            resolve_all(context, &mut function.partition_by)?;
            for order in &mut function.order_by {
                resolve_order(context, order)?;
            }
            Ok(())
        }
        SelectItem::Expr { expr, .. } => {
            *expr = resolve_exists_expr(context, expr)?;
            Ok(())
        }
    }
}

fn resolve_source(
    context: &ExistsResolutionContext<'_>,
    source: &mut QuerySource,
) -> Result<(), QueryError> {
    if let QuerySource::Join {
        left, right, on, ..
    } = source
    {
        *on = resolve_exists_expr(context, on)?;
        resolve_source(context, left)?;
        resolve_source(context, right)?;
    }
    Ok(())
}
