//! Correlated subqueries that grouping consumes row by row.
//!
//! A `GROUP BY (SELECT ...)` key and an aggregate argument such as
//! `SUM((SELECT ...))` are evaluated for every input row, before any group
//! exists. Each such subquery is evaluated once per row into a hidden
//! column, and the grouped plan reads that column instead.
use super::source::SourceExecutionEnv;
use super::{check_timeout, filter, Batch, CteContext, Expr, LogicalPlan, QueryError, SelectItem};

const COLUMN_PREFIX: &str = "__cassie_subquery_";

pub(super) fn materialize(
    env: &SourceExecutionEnv<'_>,
    ctes: &CteContext,
    plan: &LogicalPlan,
    batches: &mut [Batch],
    search: Option<&filter::SearchContext>,
) -> Result<Option<LogicalPlan>, QueryError> {
    let mut targets = Vec::new();
    for expr in &plan.group_by {
        collect(expr, &mut targets);
    }
    for expr in plan_exprs(plan) {
        aggregate_arguments(expr, &mut targets);
    }
    for item in &plan.projection {
        if let SelectItem::Function { function, .. } = item {
            if crate::sql::functions::is_aggregate_function(&function.name) {
                for arg in function.expressions() {
                    collect(arg, &mut targets);
                }
            }
        }
    }
    if targets.is_empty() {
        return Ok(None);
    }
    for batch in batches.iter_mut() {
        for row in batch.iter_mut() {
            check_timeout(env.controls)?;
            for (index, target) in targets.iter().enumerate() {
                let value =
                    super::exists_phase::evaluate(env, ctes, &plan.source, row, target, search)?;
                row.append_value(format!("{COLUMN_PREFIX}{index}"), value);
            }
        }
    }
    let mut rewritten = plan.clone();
    let replace = |expr: &Expr| rewrite(expr, &targets);
    rewritten.group_by = plan.group_by.iter().map(replace).collect();
    rewritten.having = plan.having.as_ref().map(replace);
    for order in &mut rewritten.order {
        order.expr = replace(&order.expr);
    }
    for item in &mut rewritten.projection {
        match item {
            SelectItem::Expr { expr, .. } => *expr = replace(expr),
            SelectItem::Function { function, .. } => {
                for arg in function.expressions_mut() {
                    *arg = replace(arg);
                }
            }
            SelectItem::WindowFunction { .. }
            | SelectItem::Wildcard
            | SelectItem::Column { .. } => {}
        }
    }
    Ok(Some(rewritten))
}

fn plan_exprs(plan: &LogicalPlan) -> impl Iterator<Item = &Expr> {
    plan.projection
        .iter()
        .filter_map(|item| match item {
            SelectItem::Expr { expr, .. } => Some(expr),
            _ => None,
        })
        .chain(plan.having.iter())
        .chain(plan.order.iter().map(|order| &order.expr))
}

/// Collects the unresolved subqueries of each aggregate call under `expr`.
fn aggregate_arguments(expr: &Expr, targets: &mut Vec<Expr>) {
    if let Expr::Function(function) = expr {
        if crate::sql::functions::is_aggregate_function(&function.name) {
            for arg in function.expressions() {
                collect(arg, targets);
            }
            return;
        }
    }
    expr.for_each_child(|child| aggregate_arguments(child, targets));
}

fn collect(expr: &Expr, targets: &mut Vec<Expr>) {
    if let Expr::Subquery(subquery) = expr {
        if subquery.outcome.is_none() {
            if !targets.iter().any(|target| target.structurally_eq(expr)) {
                targets.push(expr.clone());
            }
            return;
        }
    }
    expr.for_each_child(|child| collect(child, targets));
}

fn rewrite(expr: &Expr, targets: &[Expr]) -> Expr {
    if let Some(index) = targets
        .iter()
        .position(|target| target.structurally_eq(expr))
    {
        return Expr::Column(format!("{COLUMN_PREFIX}{index}"));
    }
    expr.map_children(|child| rewrite(child, targets))
}
