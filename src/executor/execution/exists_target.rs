//! PostgreSQL's `simplify_EXISTS_query`: `EXISTS` only asks whether its
//! subquery yields a row, so the target list, `GROUP BY`, `DISTINCT`,
//! `ORDER BY` and a positive constant `LIMIT` cannot change the answer and are
//! never evaluated. Aggregates, `HAVING`, window functions, set operations,
//! `OFFSET` and any other `LIMIT` can change the row count, so a subquery that
//! uses one keeps every clause.

use super::{Expr, LogicalPlan, SelectItem};
use crate::sql::ast::{OrderExpr, SelectStatement};

/// The clauses that decide whether an `EXISTS` subquery's target is relevant.
pub(super) struct ExistsClauses<'a> {
    pub(super) projection: &'a [SelectItem],
    pub(super) having: Option<&'a Expr>,
    pub(super) order: &'a [OrderExpr],
    pub(super) limit: Option<&'a Expr>,
    pub(super) offset: Option<&'a Expr>,
    pub(super) has_set: bool,
}

impl<'a> ExistsClauses<'a> {
    pub(super) fn of_plan(plan: &'a LogicalPlan) -> Self {
        Self {
            projection: &plan.projection,
            having: plan.having.as_ref(),
            order: &plan.order,
            limit: plan.limit.as_ref(),
            offset: plan.offset.as_ref(),
            has_set: plan.set.is_some(),
        }
    }

    pub(super) fn of_select(select: &'a SelectStatement) -> Self {
        Self {
            projection: &select.projection,
            having: select.having.as_ref(),
            order: &select.order,
            limit: select.limit.as_ref(),
            offset: select.offset.as_ref(),
            has_set: select.set.is_some(),
        }
    }

    /// Returns whether only row existence matters, so the target list and the
    /// clauses that depend on it can be discarded.
    pub(super) fn target_is_irrelevant(&self) -> bool {
        !self.has_set
            && self.having.is_none()
            && self.offset.is_none()
            && limit_is_irrelevant(self.limit)
            && !self.projection.iter().any(item_changes_row_count)
            && !self
                .order
                .iter()
                .any(|order| super::aggregate_exec::contains_aggregate(&order.expr))
    }
}

fn limit_is_irrelevant(limit: Option<&Expr>) -> bool {
    match limit {
        None | Some(Expr::Null) => true,
        Some(Expr::IntegerLiteral(value)) => *value > 0,
        Some(_) => false,
    }
}

fn item_changes_row_count(item: &SelectItem) -> bool {
    match item {
        SelectItem::Wildcard | SelectItem::Column { .. } => false,
        SelectItem::WindowFunction { .. } => true,
        SelectItem::Function { function, .. } => {
            crate::sql::functions::is_aggregate_function(&function.name)
                || function
                    .args
                    .iter()
                    .any(super::aggregate_exec::contains_aggregate)
        }
        SelectItem::Expr { expr, .. } => super::aggregate_exec::contains_aggregate(expr),
    }
}

/// Discards the clauses an `EXISTS` subquery cannot observe and bounds it to
/// the single row that answers it.
pub(super) fn bounded_exists_logical(mut logical: LogicalPlan) -> LogicalPlan {
    if ExistsClauses::of_plan(&logical).target_is_irrelevant() {
        // Plain column reads cannot fail, so they keep their existing read
        // shape; any computed target is replaced instead of evaluated.
        if !logical
            .projection
            .iter()
            .all(|item| matches!(item, SelectItem::Wildcard | SelectItem::Column { .. }))
        {
            logical.projection = vec![SelectItem::Expr {
                expr: Expr::IntegerLiteral(1),
                alias: None,
            }];
        }
        logical.group_by.clear();
        logical.distinct = false;
        logical.distinct_on.clear();
        logical.order.clear();
        logical.limit = None;
    }
    logical.limit = Some(Expr::IntegerLiteral(
        logical
            .limit_value()
            .map_or(1, |limit| i64::from(limit > 0)),
    ));
    logical
}
