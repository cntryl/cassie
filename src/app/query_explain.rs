use super::Cassie;

pub(crate) mod allocation;
mod nodes;
mod owned;
mod structured;
mod text;
mod text_owner;

pub use owned::{QueryExplainOutput, QueryExplainPlan};

#[cfg(test)]
mod owned_tests;

const PLAN_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct QueryPlanSummary {
    pub collection: String,
    pub root_operator: String,
    pub access_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_index: Option<String>,
    pub selected_cost: u64,
    pub estimated_rows: u64,
    pub storage_mode: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct QueryPlanNode {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub detail: String,
    pub status: String,
    pub badges: Vec<String>,
    pub metrics: Vec<QueryPlanMetric>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct QueryPlanMetric {
    pub label: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct QueryPlanAttribute {
    pub label: String,
    pub value: String,
    pub intent: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct QueryPlanEstimates {
    pub scan_rows: u64,
    pub index_rows: u64,
    pub join_rows: u64,
    pub search_rows: u64,
    pub vector_rows: u64,
    pub aggregate_rows: u64,
    pub scan_cost: u64,
    pub index_cost: u64,
    pub selected_cost: u64,
    pub cost_source: String,
    pub rejected_alternatives: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct QueryPlanFeature {
    pub id: String,
    pub label: String,
    pub enabled: bool,
    pub intent: String,
    pub detail: String,
    pub node_id: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct QueryPlanDiagnostics {
    pub access_path_reason: String,
    pub fallback_reason: String,
    pub pagination_strategy: String,
    pub early_stop: String,
    pub projection_shape: String,
    pub operator_feedback_state: String,
    pub operator_feedback_reason: String,
    pub adaptive_enabled: bool,
    pub adaptive_decision_point: String,
    pub adaptive_candidates: Vec<String>,
    pub adaptive_selected_alternative: String,
    pub adaptive_reason: String,
    pub join_strategy: String,
    pub join_fallback_reason: String,
    pub rollup_rewrite: String,
    pub projection_freshness: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct QueryPlanAnalyze {
    pub actual_rows: usize,
    pub actual_ms: u128,
    pub operator_actuals: Vec<QueryPlanOperatorActual>,
    pub diagnostics: QueryPlanAnalyzeDiagnostics,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct QueryPlanOperatorActual {
    pub operator: String,
    pub rows_in: u64,
    pub rows_out: usize,
    pub elapsed_ms: u128,
    pub storage_reads: u64,
    pub storage_writes: u64,
    pub temp_writes: u64,
    pub candidates: u64,
    pub results: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct QueryPlanAnalyzeDiagnostics {
    #[serde(rename = "plan_cache_hits_delta")]
    pub plan_cache_hits: u64,
    #[serde(rename = "plan_cache_misses_delta")]
    pub plan_cache_misses: u64,
    #[serde(rename = "storage_reads_delta")]
    pub storage_reads: u64,
    #[serde(rename = "storage_writes_delta")]
    pub storage_writes: u64,
    #[serde(rename = "temp_writes_delta")]
    pub temp_writes: u64,
    #[serde(rename = "candidate_count_delta")]
    pub candidate_count: u64,
    #[serde(rename = "result_count_delta")]
    pub result_count: u64,
    #[serde(rename = "parallel_aggregations_delta")]
    pub parallel_aggregations: u64,
    #[serde(rename = "parallel_aggregation_fallback_delta")]
    pub parallel_aggregation_fallback: u64,
    #[serde(rename = "parallel_aggregation_workers_delta")]
    pub parallel_aggregation_workers: u64,
    #[serde(rename = "parallel_aggregation_groups_delta")]
    pub parallel_aggregation_groups: u64,
    #[serde(rename = "adaptive_plan_decisions_delta")]
    pub adaptive_plan_decisions: u64,
    #[serde(rename = "adaptive_plan_selected_delta")]
    pub adaptive_plan_selected: u64,
    #[serde(rename = "operator_switch_attempts_delta")]
    pub operator_switch_attempts: u64,
    #[serde(rename = "operator_switch_success_delta")]
    pub operator_switch_success: u64,
    #[serde(rename = "operator_switch_skips_delta")]
    pub operator_switch_skips: u64,
    #[serde(rename = "operator_switch_fallbacks_delta")]
    pub operator_switch_fallbacks: u64,
}

pub(crate) use structured::structured_plan;
pub(crate) use text_owner::PlanText;

pub(super) fn plan_line(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<PlanText, super::CassieError> {
    PlanText::try_build(controls, |allocation| {
        text::plan_line(cassie, physical, allocation)
    })
}

fn selected_estimated_rows(physical: &crate::planner::physical::PhysicalPlan) -> u64 {
    if physical.read.selected_index.is_some() {
        physical.estimates.index_rows
    } else {
        physical.estimates.scan_rows
    }
}

fn storage_mode(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
) -> &'static str {
    cassie
        .catalog
        .collection_storage_mode(&physical.collection)
        .map_or("unknown", |mode| mode.as_str())
}

fn non_empty_or_none(value: &str) -> &str {
    if value.is_empty() {
        "none"
    } else {
        value
    }
}

fn selected_cost(physical: &crate::planner::physical::PhysicalPlan) -> u64 {
    let operator_feedback = non_empty_or_none(&physical.operator_feedback.state);
    let selected_alternative = non_empty_or_none(&physical.adaptive_plan.selected_alternative);
    let feedback_selected = non_empty_or_none(&physical.operator_feedback.selected_candidate);
    if operator_feedback == "used" && selected_alternative == feedback_selected {
        physical.operator_feedback.adjusted_selected_cost
    } else {
        physical.estimates.selected_cost
    }
}

fn projection_freshness(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
) -> &'static str {
    cassie
        .catalog
        .materialized_projection_freshness(&physical.collection)
}

fn rollup_rewrite(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
    allocation: &mut allocation::PlanAllocation<'_>,
) -> Result<String, super::CassieError> {
    // Existing planner discovery is an input boundary; only the final owner copy is built here.
    let name = crate::executor::rollup_rewrite_name_for_plan(cassie, &physical.logical);
    allocation.text(name.as_deref().unwrap_or("none"))
}
