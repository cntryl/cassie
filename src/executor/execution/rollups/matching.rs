//! Decides whether a rollup can answer a query plan.

use crate::catalog::{RollupAggregateMeta, RollupMeta};
use crate::planner::logical::LogicalPlan;
use crate::sql::ast::{Expr, QuerySource, SelectItem};

use super::super::source::{aggregate_signature, expr_key};
use super::Cassie;

pub(super) fn matching_rollup(
    cassie: &Cassie,
    source: &str,
    plan: &LogicalPlan,
) -> Option<RollupMeta> {
    cassie
        .catalog
        .list_rollups_for_source(source)
        .into_iter()
        .find(|rollup| rollup_matches_plan(rollup, plan))
}

fn rollup_matches_plan(rollup: &RollupMeta, plan: &LogicalPlan) -> bool {
    if rollup.version != RollupMeta::CURRENT_VERSION {
        return false;
    }
    let expected_groups = std::iter::once(rollup.bucket_expr.clone())
        .chain(rollup.group_keys.iter().cloned())
        .collect::<Vec<_>>();
    let actual_groups = plan.group_by.iter().map(expr_key).collect::<Vec<_>>();
    if actual_groups != expected_groups {
        return false;
    }
    if plan.filter.as_ref().map(expr_key) != rollup.filter_expr {
        return false;
    }
    let Some(expected_aggregates) = rollup
        .aggregates
        .iter()
        .map(stored_aggregate_signature)
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    let actual_aggregates = plan_aggregate_signatures(plan);
    actual_aggregates == expected_aggregates
}

pub(super) fn stored_aggregate_signature(aggregate: &RollupAggregateMeta) -> Option<String> {
    let Expr::Function(function) =
        crate::sql::parser::parse_expression(&aggregate.expression).ok()?
    else {
        return None;
    };
    if !function.name.eq_ignore_ascii_case(&aggregate.function) {
        return None;
    }
    Some(aggregate_signature(&function))
}

fn plan_aggregate_signatures(plan: &LogicalPlan) -> Vec<String> {
    plan.projection
        .iter()
        .filter_map(|item| match item {
            SelectItem::Function { function, .. }
                if crate::sql::functions::is_aggregate_function(&function.name) =>
            {
                Some(aggregate_signature(function))
            }
            _ => None,
        })
        .collect()
}

pub(super) fn eligible_plan_shape(plan: &LogicalPlan) -> bool {
    matches!(plan.source, QuerySource::Collection(_))
        && plan.command.is_none()
        && !plan.grouping_subtotal
        && !plan.group_by.is_empty()
        && plan.having.is_none()
        && plan.set.is_none()
        && plan.ctes.is_empty()
        && !plan.distinct
        && plan.distinct_on.is_empty()
        && !plan
            .projection
            .iter()
            .any(|item| matches!(item, SelectItem::WindowFunction { .. }))
        && aggregates_only_at_projection_top_level(plan)
}

/// A rollup stores finished aggregates, so it can only answer aggregates that
/// are whole projection items. An aggregate nested in an expression, a CASE or
/// an ORDER BY key would be evaluated against rollup rows that lack its input.
fn aggregates_only_at_projection_top_level(plan: &LogicalPlan) -> bool {
    let nests_aggregate = |expr: &Expr| {
        expr.any_descendant_or_self(&mut |expr| {
            matches!(expr, Expr::Function(function)
                if crate::sql::functions::is_aggregate_function(&function.name))
        })
    };
    plan.projection.iter().all(|item| match item {
        SelectItem::Function { function, .. } => {
            function.args.iter().all(|arg| !nests_aggregate(arg))
                && function
                    .filter
                    .as_ref()
                    .is_none_or(|filter| !nests_aggregate(filter))
        }
        SelectItem::Expr { expr, .. } => !nests_aggregate(expr),
        SelectItem::Wildcard | SelectItem::Column { .. } | SelectItem::WindowFunction { .. } => {
            true
        }
    }) && plan.order.iter().all(|order| !nests_aggregate(&order.expr))
}
