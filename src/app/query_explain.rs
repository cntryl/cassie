use super::Cassie;
use crate::executor::QueryResult;

mod text;

const PLAN_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct QueryExplainOutput {
    pub result: QueryResult,
    pub plan: QueryExplainPlan,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct QueryExplainPlan {
    pub format_version: u32,
    pub summary: QueryPlanSummary,
    pub nodes: Vec<QueryPlanNode>,
    pub attributes: Vec<QueryPlanAttribute>,
    pub estimates: QueryPlanEstimates,
    pub features: Vec<QueryPlanFeature>,
    pub diagnostics: QueryPlanDiagnostics,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analyze: Option<QueryPlanAnalyze>,
}

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

pub(crate) fn structured_plan(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
) -> QueryExplainPlan {
    QueryExplainPlan {
        format_version: PLAN_FORMAT_VERSION,
        summary: plan_summary(cassie, physical),
        nodes: plan_nodes(cassie, physical),
        attributes: plan_attributes(cassie, physical),
        estimates: plan_estimates(physical),
        features: plan_features(cassie, physical),
        diagnostics: plan_diagnostics(cassie, physical),
        analyze: None,
    }
}

pub(super) fn plan_line(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
) -> String {
    text::plan_line(cassie, physical)
}

fn plan_summary(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
) -> QueryPlanSummary {
    QueryPlanSummary {
        collection: physical.collection.clone(),
        root_operator: physical
            .operators
            .first()
            .map_or_else(|| "Command".to_string(), |operator| format!("{operator:?}")),
        access_path: physical.read.access_path.as_str().to_string(),
        selected_index: physical.read.selected_index.clone(),
        selected_cost: selected_cost(physical),
        estimated_rows: selected_estimated_rows(physical),
        storage_mode: storage_mode(cassie, physical),
    }
}

fn selected_estimated_rows(physical: &crate::planner::physical::PhysicalPlan) -> u64 {
    if physical.read.selected_index.is_some() {
        physical.estimates.index_rows
    } else {
        physical.estimates.scan_rows
    }
}

fn storage_mode(cassie: &Cassie, physical: &crate::planner::physical::PhysicalPlan) -> String {
    cassie
        .catalog
        .collection_storage_mode(&physical.collection)
        .map_or_else(|| "unknown".to_string(), |mode| mode.as_str().to_string())
}

fn plan_nodes(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
) -> Vec<QueryPlanNode> {
    let mut nodes = vec![read_node(cassie, physical)];

    if physical.join.strategy.is_some() || !physical.join.keys.is_empty() {
        nodes.push(join_node(physical));
    }

    if physical.aggregate.parallel_candidate || physical.aggregate.acceleration {
        nodes.push(aggregate_node(physical));
    }

    if physical.top_k.enabled {
        nodes.push(top_k_node(physical));
    }

    nodes.push(project_node(physical));
    nodes
}

fn read_node(cassie: &Cassie, physical: &crate::planner::physical::PhysicalPlan) -> QueryPlanNode {
    let mut badges = Vec::new();
    if let Some(index) = physical.read.selected_index.as_ref() {
        badges.push(format!("index:{index}"));
    }
    if physical.read.predicate_pushdown {
        badges.push("predicate pushdown".to_string());
    }
    if !physical.read.projected_scan_fields.is_empty() {
        badges.push("projection pruning".to_string());
    }
    if physical.read.covered_index {
        badges.push("covered index".to_string());
    }
    if let Some(index) = physical.read.column_batch_index.as_ref() {
        badges.push(format!("column batch:{index}"));
    }

    QueryPlanNode {
        id: "read".to_string(),
        label: read_node_label(physical),
        kind: "read".to_string(),
        detail: format!(
            "{} via {}",
            physical.collection,
            physical.read.access_path.as_str()
        ),
        status: read_node_status(physical),
        badges,
        metrics: vec![
            metric(
                "estimated rows",
                selected_estimated_rows(physical).to_string(),
                None,
            ),
            metric("selected cost", selected_cost(physical).to_string(), None),
            metric("storage", storage_mode(cassie, physical), None),
        ],
    }
}

fn read_node_label(physical: &crate::planner::physical::PhysicalPlan) -> String {
    match physical.read.selected_index.as_deref() {
        Some(index) => format!("Read with {index}"),
        None => "Read collection".to_string(),
    }
}

fn read_node_status(physical: &crate::planner::physical::PhysicalPlan) -> String {
    if physical.read.fallback_reason.is_some() {
        "fallback".to_string()
    } else if physical.read.selected_index.is_some()
        || physical.read.predicate_pushdown
        || physical.read.covered_index
    {
        "optimized".to_string()
    } else {
        "baseline".to_string()
    }
}

fn join_node(physical: &crate::planner::physical::PhysicalPlan) -> QueryPlanNode {
    QueryPlanNode {
        id: "join".to_string(),
        label: format!(
            "{} join",
            physical.join.strategy.as_deref().unwrap_or("runtime")
        ),
        kind: "join".to_string(),
        detail: if physical.join.keys.is_empty() {
            "No join keys captured".to_string()
        } else {
            physical.join.keys.join(", ")
        },
        status: if physical.join.fallback_reason.is_some() {
            "fallback".to_string()
        } else {
            "optimized".to_string()
        },
        badges: vec![format!("sort required:{}", physical.join.sort_required)],
        metrics: vec![metric(
            "estimated rows",
            physical.estimates.join_rows.to_string(),
            None,
        )],
    }
}

fn aggregate_node(physical: &crate::planner::physical::PhysicalPlan) -> QueryPlanNode {
    let mut badges = Vec::new();
    if physical.aggregate.parallel_candidate {
        badges.push("parallel candidate".to_string());
    }
    if physical.aggregate.acceleration {
        badges.push("accelerated".to_string());
    }

    QueryPlanNode {
        id: "aggregate".to_string(),
        label: "Aggregate".to_string(),
        kind: "aggregate".to_string(),
        detail: "Group and aggregate rows".to_string(),
        status: if physical.aggregate.acceleration {
            "optimized".to_string()
        } else {
            "baseline".to_string()
        },
        badges,
        metrics: vec![metric(
            "estimated rows",
            physical.estimates.aggregate_rows.to_string(),
            None,
        )],
    }
}

fn top_k_node(physical: &crate::planner::physical::PhysicalPlan) -> QueryPlanNode {
    QueryPlanNode {
        id: "top_k".to_string(),
        label: "Top K".to_string(),
        kind: "top_k".to_string(),
        detail: format!("{} ordering", physical.top_k.mode.as_str()),
        status: if matches!(
            physical.top_k.mode,
            crate::planner::physical::TopKMode::Storage
        ) {
            "optimized".to_string()
        } else {
            "baseline".to_string()
        },
        badges: vec![format!(
            "limit:{}",
            physical
                .top_k
                .limit
                .map_or_else(|| "none".to_string(), |limit| limit.to_string())
        )],
        metrics: vec![metric(
            "candidate budget",
            physical.top_k.limit.unwrap_or_default().to_string(),
            None,
        )],
    }
}

fn project_node(physical: &crate::planner::physical::PhysicalPlan) -> QueryPlanNode {
    QueryPlanNode {
        id: "project".to_string(),
        label: "Project rows".to_string(),
        kind: "project".to_string(),
        detail: physical.projection.shape.as_str().to_string(),
        status: "active".to_string(),
        badges: projection_badges(physical),
        metrics: vec![metric(
            "scan fields",
            if physical.read.projected_scan_fields.is_empty() {
                "all".to_string()
            } else {
                physical.read.projected_scan_fields.join(", ")
            },
            None,
        )],
    }
}

fn projection_badges(physical: &crate::planner::physical::PhysicalPlan) -> Vec<String> {
    if physical.read.projected_scan_fields.is_empty() {
        vec!["all fields".to_string()]
    } else {
        physical
            .read
            .projected_scan_fields
            .iter()
            .map(|field| format!("field:{field}"))
            .collect()
    }
}

fn plan_attributes(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
) -> Vec<QueryPlanAttribute> {
    vec![
        attribute(
            "Access path",
            physical.read.access_path.as_str(),
            attribute_intent(physical.read.selected_index.is_some()),
        ),
        attribute(
            "Index",
            physical.read.selected_index.as_deref().unwrap_or("none"),
            attribute_intent(physical.read.selected_index.is_some()),
        ),
        attribute(
            "Top K",
            physical.top_k.mode.as_str(),
            attribute_intent(physical.top_k.enabled),
        ),
        attribute(
            "Pagination",
            physical.read.pagination_strategy.as_str(),
            "neutral",
        ),
        attribute("Projection", physical.projection.shape.as_str(), "neutral"),
        attribute("Storage", storage_mode(cassie, physical), "neutral"),
        attribute(
            "Freshness",
            projection_freshness(cassie, physical),
            "neutral",
        ),
    ]
}

fn attribute(
    label: impl Into<String>,
    value: impl Into<String>,
    intent: impl Into<String>,
) -> QueryPlanAttribute {
    QueryPlanAttribute {
        label: label.into(),
        value: value.into(),
        intent: intent.into(),
    }
}

fn attribute_intent(enabled: bool) -> &'static str {
    if enabled {
        "success"
    } else {
        "neutral"
    }
}

fn metric(label: impl Into<String>, value: String, unit: Option<&str>) -> QueryPlanMetric {
    QueryPlanMetric {
        label: label.into(),
        value,
        unit: unit.map(str::to_string),
    }
}

fn plan_estimates(physical: &crate::planner::physical::PhysicalPlan) -> QueryPlanEstimates {
    let estimates = &physical.estimates;
    QueryPlanEstimates {
        scan_rows: estimates.scan_rows,
        index_rows: estimates.index_rows,
        join_rows: estimates.join_rows,
        search_rows: estimates.search_rows,
        vector_rows: estimates.vector_rows,
        aggregate_rows: estimates.aggregate_rows,
        scan_cost: estimates.scan_cost,
        index_cost: estimates.index_cost,
        selected_cost: selected_cost(physical),
        cost_source: estimates.cost_source.clone(),
        rejected_alternatives: estimates.rejected_alternatives.clone(),
    }
}

fn plan_features(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
) -> Vec<QueryPlanFeature> {
    let limits = cassie.runtime.limits();
    vec![
        feature(
            "predicate_pushdown",
            "Predicate pushdown",
            physical.read.predicate_pushdown,
            "Filters applied before rows leave storage",
            "read",
        ),
        feature(
            "projection_pruning",
            "Projection pruning",
            !physical.read.projected_scan_fields.is_empty(),
            "Read path narrows scanned fields when possible",
            "read",
        ),
        feature(
            "covered_index",
            "Covered index",
            physical.read.covered_index,
            "Selected index satisfies the requested projection",
            "read",
        ),
        feature(
            "column_batch",
            "Column batch",
            physical.read.column_batch_index.is_some(),
            "Column-batch path can serve the selected index",
            "read",
        ),
        feature(
            "top_k",
            "Top K",
            physical.top_k.enabled,
            "Ordering and limit can stop early",
            "top_k",
        ),
        feature(
            "aggregate_parallel",
            "Parallel aggregate",
            physical.aggregate.parallel_candidate,
            "Aggregate is eligible for worker parallelism",
            "aggregate",
        ),
        feature(
            "aggregate_acceleration",
            "Aggregate acceleration",
            physical.aggregate.acceleration,
            "Rollup or projection can accelerate aggregation",
            "aggregate",
        ),
        feature(
            "vectorized_join_candidate",
            "Vector join candidate",
            physical.join.vectorized.candidate,
            "Join shape can use a vectorized candidate path",
            "join",
        ),
        feature(
            "vectorized_join_enabled",
            "Vector join enabled",
            physical.join.vectorized.candidate && limits.vectorized_joins_enabled,
            "Runtime limits allow the vectorized join path",
            "join",
        ),
    ]
}

fn feature(
    id: impl Into<String>,
    label: impl Into<String>,
    enabled: bool,
    detail: impl Into<String>,
    node_id: impl Into<String>,
) -> QueryPlanFeature {
    QueryPlanFeature {
        id: id.into(),
        label: label.into(),
        enabled,
        intent: attribute_intent(enabled).to_string(),
        detail: detail.into(),
        node_id: node_id.into(),
    }
}

fn plan_diagnostics(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
) -> QueryPlanDiagnostics {
    let adaptive = &physical.adaptive_plan;
    QueryPlanDiagnostics {
        access_path_reason: physical.read.access_path_reason.clone(),
        fallback_reason: physical
            .read
            .fallback_reason
            .clone()
            .unwrap_or_else(|| "none".to_string()),
        pagination_strategy: physical.read.pagination_strategy.as_str().to_string(),
        early_stop: physical.read.early_stop.as_str().to_string(),
        projection_shape: physical.projection.shape.as_str().to_string(),
        operator_feedback_state: non_empty_or_none(&physical.operator_feedback.state).to_string(),
        operator_feedback_reason: non_empty_or_none(&physical.operator_feedback.reason).to_string(),
        adaptive_enabled: adaptive.enabled,
        adaptive_decision_point: non_empty_or_none(&adaptive.decision_point).to_string(),
        adaptive_candidates: adaptive.candidates.clone(),
        adaptive_selected_alternative: non_empty_or_none(&adaptive.selected_alternative)
            .to_string(),
        adaptive_reason: non_empty_or_none(&adaptive.reason).to_string(),
        join_strategy: physical
            .join
            .strategy
            .clone()
            .unwrap_or_else(|| "none".to_string()),
        join_fallback_reason: physical
            .join
            .fallback_reason
            .clone()
            .unwrap_or_else(|| "none".to_string()),
        rollup_rewrite: crate::executor::rollup_rewrite_name_for_plan(cassie, &physical.logical)
            .unwrap_or_else(|| "none".to_string()),
        projection_freshness: projection_freshness(cassie, physical),
    }
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
) -> String {
    cassie
        .catalog
        .get_materialized_projection(&physical.collection)
        .or_else(|| {
            cassie
                .catalog
                .materialized_projection_for_output(&physical.collection)
        })
        .map_or_else(
            || "unavailable".to_string(),
            |projection| projection.freshness.as_str().to_string(),
        )
}
