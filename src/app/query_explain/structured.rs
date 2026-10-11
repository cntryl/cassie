use crate::app::{Cassie, CassieError};
use crate::planner::physical::PhysicalPlan;
use crate::runtime::QueryExecutionControls;

use super::allocation::PlanAllocation;
use super::owned::PlanData;
use super::{
    non_empty_or_none, projection_freshness, selected_cost, QueryExplainPlan, QueryPlanAttribute,
    QueryPlanDiagnostics, QueryPlanEstimates, QueryPlanFeature, QueryPlanSummary,
    PLAN_FORMAT_VERSION,
};

pub(crate) fn structured_plan(
    cassie: &Cassie,
    physical: &PhysicalPlan,
    controls: &QueryExecutionControls,
) -> Result<QueryExplainPlan, CassieError> {
    QueryExplainPlan::try_build(controls, |allocation| {
        Ok(PlanData {
            format_version: PLAN_FORMAT_VERSION,
            summary: plan_summary(cassie, physical, allocation)?,
            nodes: super::nodes::plan_nodes(cassie, physical, allocation)?,
            attributes: plan_attributes(cassie, physical, allocation)?,
            estimates: plan_estimates(physical, allocation)?,
            features: plan_features(cassie, physical, allocation)?,
            diagnostics: plan_diagnostics(cassie, physical, allocation)?,
        })
    })
}

fn plan_summary(
    cassie: &Cassie,
    physical: &PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<QueryPlanSummary, CassieError> {
    Ok(QueryPlanSummary {
        collection: allocation.text(&physical.collection)?,
        root_operator: match physical.operators.first() {
            Some(operator) => allocation.format(format_args!("{operator:?}"))?,
            None => allocation.text("Command")?,
        },
        access_path: allocation.text(physical.read.access_path.as_str())?,
        selected_index: physical
            .read
            .selected_index
            .as_deref()
            .map(|index| allocation.text(index))
            .transpose()?,
        selected_cost: selected_cost(physical),
        estimated_rows: super::selected_estimated_rows(physical),
        storage_mode: allocation.text(super::storage_mode(cassie, physical))?,
    })
}

fn plan_attributes(
    cassie: &Cassie,
    physical: &PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<Vec<QueryPlanAttribute>, CassieError> {
    let indexed = attribute_intent(physical.read.selected_index.is_some());
    let mut values = allocation.vector(7)?;
    for (label, value, intent) in [
        ("Access path", physical.read.access_path.as_str(), indexed),
        (
            "Index",
            physical.read.selected_index.as_deref().unwrap_or("none"),
            indexed,
        ),
        (
            "Top K",
            physical.top_k.mode.as_str(),
            attribute_intent(physical.top_k.enabled),
        ),
        (
            "Pagination",
            physical.read.pagination_strategy.as_str(),
            "neutral",
        ),
        ("Projection", physical.projection.shape.as_str(), "neutral"),
        ("Storage", super::storage_mode(cassie, physical), "neutral"),
        (
            "Freshness",
            projection_freshness(cassie, physical),
            "neutral",
        ),
    ] {
        values.push(QueryPlanAttribute {
            label: allocation.text(label)?,
            value: allocation.text(value)?,
            intent: allocation.text(intent)?,
        });
    }
    Ok(values)
}

pub(super) fn attribute_intent(enabled: bool) -> &'static str {
    if enabled {
        "success"
    } else {
        "neutral"
    }
}

fn plan_estimates(
    physical: &PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<QueryPlanEstimates, CassieError> {
    let estimates = &physical.estimates;
    Ok(QueryPlanEstimates {
        scan_rows: estimates.scan_rows,
        index_rows: estimates.index_rows,
        join_rows: estimates.join_rows,
        search_rows: estimates.search_rows,
        vector_rows: estimates.vector_rows,
        aggregate_rows: estimates.aggregate_rows,
        scan_cost: estimates.scan_cost,
        index_cost: estimates.index_cost,
        selected_cost: selected_cost(physical),
        cost_source: allocation.text(&estimates.cost_source)?,
        rejected_alternatives: allocation.strings(&estimates.rejected_alternatives)?,
    })
}

fn plan_features(
    cassie: &Cassie,
    physical: &PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<Vec<QueryPlanFeature>, CassieError> {
    let limits = cassie.runtime.limits();
    let mut values = allocation.vector(9)?;
    for (id, label, enabled, detail, node_id) in [
        (
            "predicate_pushdown",
            "Predicate pushdown",
            physical.read.predicate_pushdown,
            "Filters applied before rows leave storage",
            "read",
        ),
        (
            "projection_pruning",
            "Projection pruning",
            !physical.read.projected_scan_fields.is_empty(),
            "Read path narrows scanned fields when possible",
            "read",
        ),
        (
            "covered_index",
            "Covered index",
            physical.read.covered_index,
            "Selected index satisfies the requested projection",
            "read",
        ),
        (
            "column_batch",
            "Column batch",
            physical.read.column_batch_index.is_some(),
            "Column-batch path can serve the selected index",
            "read",
        ),
        (
            "top_k",
            "Top K",
            physical.top_k.enabled,
            "Ordering and limit can stop early",
            "top_k",
        ),
        (
            "aggregate_parallel",
            "Parallel aggregate",
            physical.aggregate.parallel_candidate,
            "Aggregate is eligible for worker parallelism",
            "aggregate",
        ),
        (
            "aggregate_acceleration",
            "Aggregate acceleration",
            physical.aggregate.acceleration,
            "Rollup or projection can accelerate aggregation",
            "aggregate",
        ),
        (
            "vectorized_join_candidate",
            "Vector join candidate",
            physical.join.vectorized.candidate,
            "Join shape can use a vectorized candidate path",
            "join",
        ),
        (
            "vectorized_join_enabled",
            "Vector join enabled",
            physical.join.vectorized.candidate && limits.vectorized_joins_enabled,
            "Runtime limits allow the vectorized join path",
            "join",
        ),
    ] {
        values.push(QueryPlanFeature {
            id: allocation.text(id)?,
            label: allocation.text(label)?,
            enabled,
            intent: allocation.text(attribute_intent(enabled))?,
            detail: allocation.text(detail)?,
            node_id: allocation.text(node_id)?,
        });
    }
    Ok(values)
}

fn plan_diagnostics(
    cassie: &Cassie,
    physical: &PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<QueryPlanDiagnostics, CassieError> {
    let adaptive = &physical.adaptive_plan;
    let rewrite = super::rollup_rewrite(cassie, physical, allocation)?;
    Ok(QueryPlanDiagnostics {
        access_path_reason: allocation.text(&physical.read.access_path_reason)?,
        fallback_reason: allocation
            .text(physical.read.fallback_reason.as_deref().unwrap_or("none"))?,
        pagination_strategy: allocation.text(physical.read.pagination_strategy.as_str())?,
        early_stop: allocation.text(physical.read.early_stop.as_str())?,
        projection_shape: allocation.text(physical.projection.shape.as_str())?,
        operator_feedback_state: allocation
            .text(non_empty_or_none(&physical.operator_feedback.state))?,
        operator_feedback_reason: allocation
            .text(non_empty_or_none(&physical.operator_feedback.reason))?,
        adaptive_enabled: adaptive.enabled,
        adaptive_decision_point: allocation.text(non_empty_or_none(&adaptive.decision_point))?,
        adaptive_candidates: allocation.strings(&adaptive.candidates)?,
        adaptive_selected_alternative: allocation
            .text(non_empty_or_none(&adaptive.selected_alternative))?,
        adaptive_reason: allocation.text(non_empty_or_none(&adaptive.reason))?,
        join_strategy: allocation.text(physical.join.strategy.as_deref().unwrap_or("none"))?,
        join_fallback_reason: allocation
            .text(physical.join.fallback_reason.as_deref().unwrap_or("none"))?,
        rollup_rewrite: rewrite,
        projection_freshness: allocation.text(projection_freshness(cassie, physical))?,
    })
}
