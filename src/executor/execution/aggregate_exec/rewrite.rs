//! Rewrites expressions evaluated over aggregated group rows.
//!
//! After aggregation each group row holds its `GROUP BY` values (keyed by
//! [`group_expr_name`]) and every aggregate result (keyed by
//! [`aggregate_signature`]). Projection, `HAVING`, and nested aggregate
//! expressions such as `CASE WHEN SUM(x) > 0 ...` or `COALESCE(COUNT(x), 0)`
//! are rewritten to read those columns instead of re-evaluating per-row
//! sub-expressions.

use crate::sql::ast::{Expr, FunctionCall, SelectItem};
use crate::sql::functions::is_aggregate_function;

use super::super::{aggregate_signature, group_expr_name};

/// Replaces grouped sub-expressions and aggregate calls with references to
/// the group row columns that hold their values.
pub(in crate::executor::execution) fn rewrite_aggregate_expr(
    expr: &Expr,
    group_by: &[Expr],
) -> Expr {
    if let Some(grouped) = group_by
        .iter()
        .find(|grouped| grouped.structurally_eq(expr))
    {
        return Expr::Column(group_expr_name(grouped));
    }
    match expr {
        Expr::Function(function) if is_aggregate_function(&function.name) => {
            Expr::Column(aggregate_signature(function))
        }
        _ => expr.map_children(|child| rewrite_aggregate_expr(child, group_by)),
    }
}

/// Returns whether `expr` calls an aggregate function anywhere inside it.
pub(in crate::executor::execution) fn contains_aggregate(expr: &Expr) -> bool {
    expr.any_descendant_or_self(&mut |expr| {
        matches!(expr, Expr::Function(function) if is_aggregate_function(&function.name))
    })
}

/// Rewrites projection items whose expressions read grouped values or nest
/// aggregates, so they evaluate against aggregated group rows.
pub(in crate::executor::execution) fn rewrite_aggregate_projection(
    projection: &[SelectItem],
    group_by: &[Expr],
) -> Vec<SelectItem> {
    projection
        .iter()
        .map(|item| match item {
            SelectItem::Expr { expr, alias } => SelectItem::Expr {
                expr: rewrite_aggregate_expr(expr, group_by),
                alias: alias.clone(),
            },
            SelectItem::Function { function, alias } if !is_aggregate_function(&function.name) => {
                SelectItem::Function {
                    function: FunctionCall {
                        name: function.name.clone(),
                        args: function
                            .args
                            .iter()
                            .map(|arg| rewrite_aggregate_expr(arg, group_by))
                            .collect(),
                    },
                    alias: alias.clone(),
                }
            }
            SelectItem::Wildcard
            | SelectItem::Column { .. }
            | SelectItem::Function { .. }
            | SelectItem::WindowFunction { .. } => item.clone(),
        })
        .collect()
}
