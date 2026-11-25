use crate::catalog::RollupMeta;
use crate::planner::logical::LogicalPlan;
use crate::sql::ast::{Expr, OrderExpr};

use super::super::source::{aggregate_signature, expr_key};

/// Rewrites query ordering to the stored rollup output columns.
///
/// Rollup output rows keep the bucket under its canonical expression text and
/// each aggregate under its stored alias. The source columns those expressions
/// name are absent, so evaluating the original expressions would order by NULL.
pub(super) fn rollup_output_order(rollup: &RollupMeta, plan: &LogicalPlan) -> Vec<OrderExpr> {
    plan.order
        .iter()
        .map(|order| {
            let resolved = crate::executor::sort::alias_expr(&order.expr, &plan.projection)
                .unwrap_or_else(|| order.expr.clone());
            OrderExpr {
                expr: output_column(rollup, &resolved).unwrap_or(resolved),
                direction: order.direction.clone(),
                nulls: order.nulls,
            }
        })
        .collect()
}

fn output_column(rollup: &RollupMeta, expr: &Expr) -> Option<Expr> {
    if expr_key(expr) == rollup.bucket_expr {
        return Some(Expr::Column(rollup.bucket_expr.clone()));
    }
    let Expr::Function(function) = expr else {
        return None;
    };
    let signature = aggregate_signature(function);
    rollup
        .aggregates
        .iter()
        .find(|aggregate| {
            super::matching::stored_aggregate_signature(aggregate).as_ref() == Some(&signature)
        })
        .map(|aggregate| Expr::Column(aggregate.alias.clone()))
}
