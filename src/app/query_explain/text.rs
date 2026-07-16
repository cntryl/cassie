use super::super::{vector_prefilter_fallback_reason, vector_prefilter_supported, Cassie};
use super::{non_empty_or_none, projection_freshness, selected_cost};

pub(super) fn plan_line(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
) -> String {
    [
        read_plan_section(cassie, physical),
        operator_feedback_section(physical),
        adaptive_plan_section(physical),
        phase03_section(cassie, physical),
        join_section(physical),
        vectorized_join_section(cassie, physical),
        acceleration_section(cassie, physical),
        cost_section(physical),
    ]
    .join(" ")
}

fn read_plan_section(cassie: &Cassie, physical: &crate::planner::physical::PhysicalPlan) -> String {
    let projection_pruning = !physical.read.projected_scan_fields.is_empty();
    let scan_fields = if projection_pruning {
        physical.read.projected_scan_fields.join(",")
    } else {
        "all".to_string()
    };
    let scan_limit = physical
        .read
        .scan_limit
        .map_or_else(|| "none".to_string(), |limit| limit.to_string());
    let storage_mode = cassie
        .catalog
        .collection_storage_mode(&physical.collection)
        .map_or_else(|| "unknown".to_string(), |mode| mode.as_str().to_string());
    let selected_index = physical.read.selected_index.as_deref().unwrap_or("none");
    format!(
        "collection={} operators={} predicate_pushdown={} projection_pruning={} scan_fields={} limit_pushdown={} scan_limit={} access_path={} access_path_reason={} fallback_reason={} pagination_strategy={} top_k_mode={} early_stop={} projection_shape={} storage_mode={} index_aware={} index={} index_feedback={}",
        physical.collection,
        operators_description(physical),
        physical.read.predicate_pushdown,
        projection_pruning,
        scan_fields,
        physical.read.scan_limit.is_some(),
        scan_limit,
        physical.read.access_path.as_str(),
        physical.read.access_path_reason.as_str(),
        physical.read.fallback_reason.as_deref().unwrap_or("none"),
        physical.read.pagination_strategy.as_str(),
        physical.top_k.mode.as_str(),
        physical.read.early_stop.as_str(),
        physical.projection.shape.as_str(),
        storage_mode,
        physical.read.selected_index.is_some(),
        selected_index,
        index_feedback(physical)
    )
}

fn operators_description(physical: &crate::planner::physical::PhysicalPlan) -> String {
    let operators = physical
        .operators
        .iter()
        .map(|operator| format!("{operator:?}"))
        .collect::<Vec<_>>()
        .join(">");
    if operators.is_empty() {
        "Command".to_string()
    } else {
        operators
    }
}

fn index_feedback(physical: &crate::planner::physical::PhysicalPlan) -> &'static str {
    if physical.read.selected_index.is_some() {
        "enabled"
    } else {
        "none"
    }
}

fn operator_feedback_section(physical: &crate::planner::physical::PhysicalPlan) -> String {
    let feedback = &physical.operator_feedback;
    format!(
        "operator_feedback={} operator_feedback_reason={} operator_feedback_base_candidate={} operator_feedback_selected_candidate={} operator_feedback_base_cost={} operator_feedback_adjusted_cost={} operator_feedback_confidence_bps={} operator_feedback_age_ms={} operator_feedback_samples={} operator_feedback_outliers={}",
        non_empty_or_none(&feedback.state),
        non_empty_or_none(&feedback.reason),
        non_empty_or_none(&feedback.base_candidate),
        non_empty_or_none(&feedback.selected_candidate),
        feedback.base_selected_cost,
        feedback.adjusted_selected_cost,
        feedback.confidence_bps,
        feedback.age_ms,
        feedback.samples,
        feedback.outlier_samples
    )
}

fn adaptive_plan_section(physical: &crate::planner::physical::PhysicalPlan) -> String {
    let adaptive_plan = &physical.adaptive_plan;
    let adaptive_candidates = if adaptive_plan.candidates.is_empty() {
        "none".to_string()
    } else {
        adaptive_plan.candidates.join("|")
    };
    format!(
        "adaptive_plan_enabled={} adaptive_decision_point={} adaptive_candidates={} adaptive_base_alternative={} adaptive_selected_alternative={} adaptive_guard={} adaptive_guard_passed={} adaptive_reason={} adaptive_diagnostic={}",
        adaptive_plan.enabled,
        non_empty_or_none(&adaptive_plan.decision_point),
        adaptive_candidates,
        non_empty_or_none(&adaptive_plan.base_alternative),
        non_empty_or_none(&adaptive_plan.selected_alternative),
        non_empty_or_none(&adaptive_plan.guard),
        adaptive_plan.guard_passed,
        non_empty_or_none(&adaptive_plan.reason),
        non_empty_or_none(&adaptive_plan.diagnostic)
    )
}

fn phase03_section(cassie: &Cassie, physical: &crate::planner::physical::PhysicalPlan) -> String {
    let diagnostics = phase03_diagnostics(cassie, physical);
    let top_k_limit = physical
        .top_k
        .limit
        .map_or_else(|| "none".to_string(), |limit| limit.to_string());
    format!(
        "covered_index={} column_batch_index={} column_native={} hybrid_row_column={} vectorized_aggregate={} parallel_pipeline={} analytical_projection={} prefilter={} time_series={} time_series_storage={} top_k={} top_k_limit={} candidate_budget={}",
        physical.read.covered_index,
        physical.read.column_batch_index.as_deref().unwrap_or("none"),
        diagnostics.column_native,
        diagnostics.hybrid_row_column,
        diagnostics.vectorized_aggregate,
        diagnostics.parallel_pipeline,
        diagnostics.analytical_projection,
        prefilter_description(cassie, physical),
        time_series_description(cassie, physical),
        time_series_storage_description(cassie, physical),
        physical.top_k.enabled,
        top_k_limit,
        candidate_budget(cassie, physical)
    )
}

fn join_section(physical: &crate::planner::physical::PhysicalPlan) -> String {
    let join_keys = if physical.join.keys.is_empty() {
        "none".to_string()
    } else {
        physical.join.keys.join(",")
    };
    let join_order = if physical.join.order.is_empty() {
        "none".to_string()
    } else {
        physical.join.order.join(">")
    };
    let legality_barriers = if physical.join.legality_barriers.is_empty() {
        "none".to_string()
    } else {
        physical.join.legality_barriers.join(",")
    };
    let required_columns = if physical.join.properties.required_columns.is_empty() {
        "none".to_string()
    } else {
        physical.join.properties.required_columns.join(",")
    };
    let required_ordering = if physical.join.properties.required_ordering.is_empty() {
        "none".to_string()
    } else {
        physical.join.properties.required_ordering.join(",")
    };
    format!(
        "join_strategy={} join_enumeration={} join_order={} join_keys={} join_sort_required={} join_fallback_reason={} join_legality_barriers={} join_required_columns={} join_required_ordering={} join_parameterized={} join_rewindable={} join_bounded={} join_memory_bound={}",
        physical.join.strategy.as_deref().unwrap_or("none"),
        physical.join.enumeration,
        join_order,
        join_keys,
        physical.join.sort_required,
        physical.join.fallback_reason.as_deref().unwrap_or("none"),
        legality_barriers,
        required_columns,
        required_ordering,
        physical.join.properties.parameterized,
        physical.join.properties.rewindable,
        physical.join.properties.bounded,
        physical.join.properties.memory_bound
    )
}

fn vectorized_join_section(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
) -> String {
    let limits = cassie.runtime.limits();
    let vectorized = vectorized_join_status(physical, &limits);
    let operator_switch = operator_switch_status(physical, &limits);
    format!(
        "vectorized_join_candidate={} vectorized_join_enabled={} vectorized_join_batch_size={} vectorized_join_fallback_reason={} operator_switch_candidate={} operator_switch_enabled={} operator_switch_pair={} operator_switch_threshold={} operator_switch_reason={}",
        vectorized.candidate,
        vectorized.enabled,
        vectorized.batch_size,
        vectorized.fallback_reason,
        operator_switch.candidate,
        operator_switch.enabled,
        operator_switch.pair,
        operator_switch.threshold,
        operator_switch.reason
    )
}

fn acceleration_section(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
) -> String {
    let mixed = mixed_execution_diagnostics(physical);
    format!(
        "aggregate_parallel={} aggregate_acceleration={} rollup_rewrite={} mixed_execution={} mixed_stages={} exact_baseline={} projection_freshness={}",
        physical.aggregate.parallel_candidate,
        physical.aggregate.acceleration,
        crate::executor::rollup_rewrite_name_for_plan(cassie, &physical.logical)
            .unwrap_or_else(|| "none".to_string()),
        mixed.enabled,
        mixed.stages,
        mixed.exact_baseline,
        projection_freshness(cassie, physical)
    )
}

fn cost_section(physical: &crate::planner::physical::PhysicalPlan) -> String {
    let estimates = &physical.estimates;
    let rejected_alternatives = if estimates.rejected_alternatives.is_empty() {
        "none".to_string()
    } else {
        estimates.rejected_alternatives.join(",")
    };
    format!(
        "cost_model=v{} selected_cost={} scan_cost={} index_cost={} cost_source={} rejected_alternatives={} estimates=scan:{} index:{} join:{} search:{} vector:{} aggregate:{}",
        estimates.cost_model_version,
        selected_cost(physical),
        estimates.scan_cost,
        estimates.index_cost,
        estimates.cost_source,
        rejected_alternatives,
        estimates.scan_rows,
        estimates.index_rows,
        estimates.join_rows,
        estimates.search_rows,
        estimates.vector_rows,
        estimates.aggregate_rows
    )
}

struct VectorizedJoinStatus {
    candidate: bool,
    enabled: bool,
    batch_size: usize,
    fallback_reason: String,
}

fn vectorized_join_status(
    physical: &crate::planner::physical::PhysicalPlan,
    limits: &crate::config::CassieRuntimeLimits,
) -> VectorizedJoinStatus {
    let candidate = physical.join.vectorized.candidate;
    let enabled = candidate && limits.vectorized_joins_enabled;
    let fallback_reason = if enabled {
        "none".to_string()
    } else if candidate {
        "disabled".to_string()
    } else {
        physical
            .join
            .vectorized
            .fallback_reason
            .clone()
            .unwrap_or_else(|| "none".to_string())
    };
    VectorizedJoinStatus {
        candidate,
        enabled,
        batch_size: limits.vectorized_join_batch_size.max(1),
        fallback_reason,
    }
}

struct OperatorSwitchStatus {
    candidate: bool,
    enabled: bool,
    pair: &'static str,
    threshold: usize,
    reason: &'static str,
}

fn operator_switch_status(
    physical: &crate::planner::physical::PhysicalPlan,
    limits: &crate::config::CassieRuntimeLimits,
) -> OperatorSwitchStatus {
    let candidate = physical.join.vectorized.candidate;
    let enabled = candidate && limits.operator_switching_enabled.is_enabled();
    OperatorSwitchStatus {
        candidate,
        enabled,
        pair: if candidate {
            "vectorized_join_to_merge_join"
        } else {
            "none"
        },
        threshold: limits.operator_switch_join_row_threshold,
        reason: operator_switch_reason(candidate, enabled),
    }
}

fn operator_switch_reason(candidate: bool, enabled: bool) -> &'static str {
    if enabled {
        "armed"
    } else if candidate {
        "disabled"
    } else {
        "not_prevalidated"
    }
}

fn prefilter_description(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
) -> String {
    match physical.logical.filter.as_ref() {
        None => "none".to_string(),
        Some(filter) => {
            if let Some(index) = physical.read.selected_index.as_deref() {
                format!("index={index}")
            } else if let Some(schema) = cassie.catalog.get_schema(&physical.collection) {
                if vector_prefilter_supported(filter, &schema) {
                    "row-scan".to_string()
                } else {
                    format!(
                        "fallback={}",
                        vector_prefilter_fallback_reason(filter, &schema)
                    )
                }
            } else {
                "fallback=missing-schema".to_string()
            }
        }
    }
}

fn time_series_description(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
) -> String {
    let Some(index_name) = physical.read.selected_index.as_deref() else {
        return "none".to_string();
    };
    let Some(index) = cassie.catalog.get_index(&physical.collection, index_name) else {
        return "none".to_string();
    };
    if index.kind != crate::catalog::IndexKind::TimeSeries {
        return "none".to_string();
    }
    let bucket_width = index
        .options
        .get("bucket_width")
        .cloned()
        .unwrap_or_else(|| "none".to_string());
    let partition_by = index
        .options
        .get("partition_by")
        .cloned()
        .unwrap_or_else(|| "none".to_string());
    let range_filter = physical.logical.filter.is_some();
    format!("bucket_width:{bucket_width},partition_by:{partition_by},range_filter:{range_filter}")
}

fn time_series_storage_description(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
) -> String {
    let Some(index_name) = physical.read.selected_index.as_deref() else {
        return "none".to_string();
    };
    let Some(index) = cassie.catalog.get_index(&physical.collection, index_name) else {
        return "none".to_string();
    };
    if index.kind != crate::catalog::IndexKind::TimeSeries {
        return "none".to_string();
    }
    if time_series_bucket_width_supported(index.options.get("bucket_width").map(String::as_str)) {
        "bucket-native-v1".to_string()
    } else {
        "row-backed-fallback".to_string()
    }
}

fn time_series_bucket_width_supported(raw: Option<&str>) -> bool {
    let Some(raw) = raw else {
        return false;
    };
    let mut parts = raw.split_whitespace();
    let amount = parts.next().and_then(|value| value.parse::<u64>().ok());
    let unit = parts.next().map(str::to_ascii_lowercase);
    amount.is_some_and(|value| value > 0)
        && parts.next().is_none()
        && matches!(
            unit.as_deref(),
            Some("minute" | "minutes" | "hour" | "hours" | "day" | "days")
        )
}

fn candidate_budget(cassie: &Cassie, physical: &crate::planner::physical::PhysicalPlan) -> String {
    physical
        .top_k
        .limit
        .map(|top_needed| {
            let limits = cassie.runtime.limits();
            let feedback_budget = cassie
                .runtime
                .feedback_candidate_budget(&physical.collection)
                .unwrap_or_default();
            top_needed
                .max(limits.adaptive_candidate_min)
                .max(feedback_budget)
                .min(limits.adaptive_candidate_max)
        })
        .map_or_else(|| "none".to_string(), |budget| budget.to_string())
}

#[derive(Clone, Copy)]
enum DiagnosticFlag {
    Enabled,
    Disabled,
}

impl DiagnosticFlag {
    const fn from_bool(value: bool) -> Self {
        if value {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }

    const fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

impl std::fmt::Display for DiagnosticFlag {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Enabled => "true",
            Self::Disabled => "false",
        })
    }
}

struct Phase03Diagnostics {
    column_native: DiagnosticFlag,
    hybrid_row_column: DiagnosticFlag,
    vectorized_aggregate: DiagnosticFlag,
    parallel_pipeline: DiagnosticFlag,
    analytical_projection: String,
}

fn phase03_diagnostics(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
) -> Phase03Diagnostics {
    let column_native = physical.read.column_batch_index.is_some();
    let hybrid_row_column = column_native
        && (physical.logical.filter.is_some()
            || !physical.logical.order.is_empty()
            || !physical.logical.projection.is_empty());
    let parallel_pipeline = physical.aggregate.parallel_candidate
        || physical
            .operators
            .iter()
            .any(|operator| matches!(operator, crate::planner::physical::Operator::VectorSearch));
    let analytical_projection = cassie
        .catalog
        .list_projection_metadata()
        .into_iter()
        .filter_map(|metadata| metadata.materialized.map(|mat| (metadata.collection, mat)))
        .find(|(_, materialized)| {
            materialized
                .options
                .get("analytical")
                .is_some_and(|value| value.eq_ignore_ascii_case("true"))
                && materialized
                    .source_collections
                    .iter()
                    .any(|source| source == &physical.collection)
        })
        .map_or_else(|| "none".to_string(), |(name, _)| name);

    Phase03Diagnostics {
        column_native: DiagnosticFlag::from_bool(column_native),
        hybrid_row_column: DiagnosticFlag::from_bool(hybrid_row_column),
        vectorized_aggregate: DiagnosticFlag::from_bool(physical.aggregate.acceleration),
        parallel_pipeline: DiagnosticFlag::from_bool(parallel_pipeline),
        analytical_projection,
    }
}

struct MixedExecutionDiagnostics {
    enabled: bool,
    stages: String,
    exact_baseline: &'static str,
}

fn mixed_execution_diagnostics(
    physical: &crate::planner::physical::PhysicalPlan,
) -> MixedExecutionDiagnostics {
    let inputs = MixedExecutionInputs::from_plan(physical);
    let enabled = inputs.is_enabled();
    let stages = mixed_execution_stages(&inputs);

    MixedExecutionDiagnostics {
        enabled,
        stages,
        exact_baseline: if enabled {
            "source_row_exact_baseline"
        } else {
            "none"
        },
    }
}

struct MixedExecutionInputs {
    operators: MixedOperatorUse,
    clauses: MixedClauseUse,
}

struct MixedOperatorUse {
    fulltext: DiagnosticFlag,
    vector: DiagnosticFlag,
    aggregate: DiagnosticFlag,
}

struct MixedClauseUse {
    filter: DiagnosticFlag,
    order: DiagnosticFlag,
    offset: DiagnosticFlag,
    limit: DiagnosticFlag,
}

impl MixedExecutionInputs {
    fn from_plan(physical: &crate::planner::physical::PhysicalPlan) -> Self {
        Self {
            operators: MixedOperatorUse {
                fulltext: DiagnosticFlag::from_bool(has_operator(
                    physical,
                    &crate::planner::physical::Operator::FullTextSearch,
                )),
                vector: DiagnosticFlag::from_bool(has_operator(
                    physical,
                    &crate::planner::physical::Operator::VectorSearch,
                )),
                aggregate: DiagnosticFlag::from_bool(has_operator(
                    physical,
                    &crate::planner::physical::Operator::Aggregate,
                )),
            },
            clauses: MixedClauseUse {
                filter: DiagnosticFlag::from_bool(physical.logical.filter.is_some()),
                order: DiagnosticFlag::from_bool(!physical.logical.order.is_empty()),
                offset: DiagnosticFlag::from_bool(physical.logical.offset.is_some()),
                limit: DiagnosticFlag::from_bool(physical.logical.limit.is_some()),
            },
        }
    }

    fn is_enabled(&self) -> bool {
        let uses_fulltext = self.operators.fulltext.is_enabled();
        let uses_vector = self.operators.vector.is_enabled();
        let uses_aggregate = self.operators.aggregate.is_enabled();
        (uses_fulltext && uses_vector) || ((uses_fulltext || uses_vector) && uses_aggregate)
    }
}

fn has_operator(
    physical: &crate::planner::physical::PhysicalPlan,
    target: &crate::planner::physical::Operator,
) -> bool {
    physical.operators.contains(target)
}

fn mixed_execution_stages(inputs: &MixedExecutionInputs) -> String {
    let mut stages = Vec::new();
    let uses_search =
        inputs.operators.fulltext.is_enabled() || inputs.operators.vector.is_enabled();
    if uses_search {
        stages.push("candidate_generation");
    }
    if inputs.clauses.filter.is_enabled() {
        stages.push("metadata_prefilter");
    }
    if uses_search {
        stages.push("exact_scoring");
    }
    if inputs.operators.aggregate.is_enabled() {
        stages.push("analytical_grouping");
    }
    if inputs.clauses.order.is_enabled() {
        stages.push("ordering");
    }
    if inputs.clauses.offset.is_enabled() {
        stages.push("offset");
    }
    if inputs.clauses.limit.is_enabled() {
        stages.push("limit");
    }
    if stages.is_empty() {
        "none".to_string()
    } else {
        stages.join(">")
    }
}
