use super::{Cassie, vector_prefilter_supported, vector_prefilter_fallback_reason};

pub(super) fn plan_line(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
) -> String {
    let operators = physical
        .operators
        .iter()
        .map(|operator| format!("{operator:?}"))
        .collect::<Vec<_>>()
        .join(">");
    let projection_pruning = !physical.projected_scan_fields.is_empty();
    let scan_fields = if projection_pruning {
        physical.projected_scan_fields.join(",")
    } else {
        "all".to_string()
    };
    let limit_pushdown = physical.scan_limit.is_some();
    let scan_limit = physical
        .scan_limit.map_or_else(|| "none".to_string(), |limit| limit.to_string());
    let index_aware = physical.selected_index.is_some();
    let index = physical.selected_index.as_deref().unwrap_or("none");
    let index_feedback = if physical.selected_index.is_some() {
        "enabled"
    } else {
        "none"
    };
    let operator_feedback = if physical.operator_feedback.state.is_empty() {
        "none"
    } else {
        physical.operator_feedback.state.as_str()
    };
    let operator_feedback_reason = if physical.operator_feedback.reason.is_empty() {
        "none"
    } else {
        physical.operator_feedback.reason.as_str()
    };
    let operator_feedback_base_candidate = if physical.operator_feedback.base_candidate.is_empty() {
        "none"
    } else {
        physical.operator_feedback.base_candidate.as_str()
    };
    let operator_feedback_selected_candidate =
        if physical.operator_feedback.selected_candidate.is_empty() {
            "none"
        } else {
            physical.operator_feedback.selected_candidate.as_str()
        };
    let adaptive_plan = &physical.adaptive_plan;
    let adaptive_plan_enabled = adaptive_plan.enabled;
    let adaptive_decision_point = if adaptive_plan.decision_point.is_empty() {
        "none"
    } else {
        adaptive_plan.decision_point.as_str()
    };
    let adaptive_candidates = if adaptive_plan.candidates.is_empty() {
        "none".to_string()
    } else {
        adaptive_plan.candidates.join("|")
    };
    let adaptive_base_alternative = if adaptive_plan.base_alternative.is_empty() {
        "none"
    } else {
        adaptive_plan.base_alternative.as_str()
    };
    let adaptive_selected_alternative = if adaptive_plan.selected_alternative.is_empty() {
        "none"
    } else {
        adaptive_plan.selected_alternative.as_str()
    };
    let adaptive_guard = if adaptive_plan.guard.is_empty() {
        "none"
    } else {
        adaptive_plan.guard.as_str()
    };
    let adaptive_reason = if adaptive_plan.reason.is_empty() {
        "none"
    } else {
        adaptive_plan.reason.as_str()
    };
    let adaptive_diagnostic = if adaptive_plan.diagnostic.is_empty() {
        "none"
    } else {
        adaptive_plan.diagnostic.as_str()
    };
    let covered_index = physical.covered_index;
    let column_batch_index = physical.column_batch_index.as_deref().unwrap_or("none");
    let prefilter = prefilter_description(cassie, physical);
    let time_series = time_series_description(cassie, physical);
    let time_series_storage = time_series_storage_description(cassie, physical);
    let top_k_limit = physical
        .top_k_limit.map_or_else(|| "none".to_string(), |limit| limit.to_string());
    let access_path = physical.access_path.as_str();
    let access_path_reason = physical.access_path_reason.as_str();
    let fallback_reason = physical.fallback_reason.as_deref().unwrap_or("none");
    let pagination_strategy = physical.pagination_strategy.as_str();
    let top_k_mode = physical.top_k_mode.as_str();
    let early_stop = physical.early_stop.as_str();
    let projection_shape = physical.projection_shape.as_str();
    let join_strategy = physical.join_strategy.as_deref().unwrap_or("none");
    let join_keys = if physical.join_keys.is_empty() {
        "none".to_string()
    } else {
        physical.join_keys.join(",")
    };
    let join_sort_required = physical.join_sort_required;
    let join_fallback_reason = physical.join_fallback_reason.as_deref().unwrap_or("none");
    let limits = cassie.runtime.limits();
    let vectorized_join_candidate = physical.vectorized_join_candidate;
    let vectorized_join_enabled = vectorized_join_candidate && limits.vectorized_joins_enabled;
    let vectorized_join_batch_size = limits.vectorized_join_batch_size.max(1);
    let vectorized_join_fallback_reason = if vectorized_join_enabled {
        "none"
    } else if vectorized_join_candidate {
        "disabled"
    } else {
        physical
            .vectorized_join_fallback_reason
            .as_deref()
            .unwrap_or("none")
    };
    let operator_switch_candidate = vectorized_join_candidate;
    let operator_switch_enabled = operator_switch_candidate && limits.operator_switching_enabled;
    let operator_switch_pair = if operator_switch_candidate {
        "vectorized_join_to_merge_join"
    } else {
        "none"
    };
    let operator_switch_threshold = limits.operator_switch_join_row_threshold;
    let operator_switch_reason = if operator_switch_enabled {
        "armed"
    } else if operator_switch_candidate {
        "disabled"
    } else {
        "not_prevalidated"
    };
    let storage_mode = cassie
        .catalog
        .collection_storage_mode(&physical.collection).map_or_else(|| "unknown".to_string(), |mode| mode.as_str().to_string());
    let aggregate_parallel = physical.parallel_aggregate_candidate;
    let aggregate_acceleration = physical.aggregate_acceleration;
    let rollup_rewrite = crate::executor::rollup_rewrite_name_for_plan(cassie, &physical.logical)
        .unwrap_or_else(|| "none".to_string());
    let diagnostics = phase03_diagnostics(cassie, physical);
    let candidate_budget = candidate_budget(cassie, physical);
    let mixed = mixed_execution_diagnostics(physical);
    let projection_freshness = cassie
        .catalog
        .get_materialized_projection(&physical.collection)
        .or_else(|| {
            cassie
                .catalog
                .materialized_projection_for_output(&physical.collection)
        }).map_or_else(|| "unavailable".to_string(), |projection| projection.freshness.as_str().to_string());
    let estimates = &physical.estimates;
    let selected_cost = if operator_feedback == "used"
        && adaptive_selected_alternative == operator_feedback_selected_candidate
    {
        physical.operator_feedback.adjusted_selected_cost
    } else {
        estimates.selected_cost
    };
    let rejected_alternatives = if estimates.rejected_alternatives.is_empty() {
        "none".to_string()
    } else {
        estimates.rejected_alternatives.join(",")
    };

    format!(
        "collection={} operators={} predicate_pushdown={} projection_pruning={} scan_fields={} limit_pushdown={} scan_limit={} access_path={} access_path_reason={} fallback_reason={} pagination_strategy={} top_k_mode={} early_stop={} projection_shape={} storage_mode={} index_aware={} index={} index_feedback={} operator_feedback={} operator_feedback_reason={} operator_feedback_base_candidate={} operator_feedback_selected_candidate={} operator_feedback_base_cost={} operator_feedback_adjusted_cost={} operator_feedback_confidence_bps={} operator_feedback_age_ms={} operator_feedback_samples={} operator_feedback_outliers={} adaptive_plan_enabled={} adaptive_decision_point={} adaptive_candidates={} adaptive_base_alternative={} adaptive_selected_alternative={} adaptive_guard={} adaptive_guard_passed={} adaptive_reason={} adaptive_diagnostic={} covered_index={} column_batch_index={} column_native={} hybrid_row_column={} vectorized_aggregate={} parallel_pipeline={} analytical_projection={} prefilter={} time_series={} time_series_storage={} top_k={} top_k_limit={} candidate_budget={} join_strategy={} join_keys={} join_sort_required={} join_fallback_reason={} vectorized_join_candidate={} vectorized_join_enabled={} vectorized_join_batch_size={} vectorized_join_fallback_reason={} operator_switch_candidate={} operator_switch_enabled={} operator_switch_pair={} operator_switch_threshold={} operator_switch_reason={} aggregate_parallel={} aggregate_acceleration={} rollup_rewrite={} mixed_execution={} mixed_stages={} exact_baseline={} projection_freshness={} cost_model=v{} selected_cost={} scan_cost={} index_cost={} cost_source={} rejected_alternatives={} estimates=scan:{} index:{} join:{} search:{} vector:{} aggregate:{}",
        physical.collection,
        if operators.is_empty() {
            "Command".to_string()
        } else {
            operators
        },
        physical.predicate_pushdown,
        projection_pruning,
        scan_fields,
        limit_pushdown,
        scan_limit,
        access_path,
        access_path_reason,
        fallback_reason,
        pagination_strategy,
        top_k_mode,
        early_stop,
        projection_shape,
        storage_mode,
        index_aware,
        index,
        index_feedback,
        operator_feedback,
        operator_feedback_reason,
        operator_feedback_base_candidate,
        operator_feedback_selected_candidate,
        physical.operator_feedback.base_selected_cost,
        physical.operator_feedback.adjusted_selected_cost,
        physical.operator_feedback.confidence_bps,
        physical.operator_feedback.age_ms,
        physical.operator_feedback.samples,
        physical.operator_feedback.outlier_samples,
        adaptive_plan_enabled,
        adaptive_decision_point,
        adaptive_candidates,
        adaptive_base_alternative,
        adaptive_selected_alternative,
        adaptive_guard,
        adaptive_plan.guard_passed,
        adaptive_reason,
        adaptive_diagnostic,
        covered_index,
        column_batch_index,
        diagnostics.column_native,
        diagnostics.hybrid_row_column,
        diagnostics.vectorized_aggregate,
        diagnostics.parallel_pipeline,
        diagnostics.analytical_projection,
        prefilter,
        time_series,
        time_series_storage,
        physical.top_k,
        top_k_limit,
        candidate_budget,
        join_strategy,
        join_keys,
        join_sort_required,
        join_fallback_reason,
        vectorized_join_candidate,
        vectorized_join_enabled,
        vectorized_join_batch_size,
        vectorized_join_fallback_reason,
        operator_switch_candidate,
        operator_switch_enabled,
        operator_switch_pair,
        operator_switch_threshold,
        operator_switch_reason,
        aggregate_parallel,
        aggregate_acceleration,
        rollup_rewrite,
        mixed.enabled,
        mixed.stages,
        mixed.exact_baseline,
        projection_freshness,
        estimates.cost_model_version,
        selected_cost,
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

fn prefilter_description(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
) -> String {
    match physical.logical.filter.as_ref() {
        None => "none".to_string(),
        Some(filter) => {
            if let Some(index) = physical.selected_index.as_deref() {
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
    let Some(index_name) = physical.selected_index.as_deref() else {
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
    let Some(index_name) = physical.selected_index.as_deref() else {
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
        .top_k_limit
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
        }).map_or_else(|| "none".to_string(), |budget| budget.to_string())
}

struct Phase03Diagnostics {
    column_native: bool,
    hybrid_row_column: bool,
    vectorized_aggregate: bool,
    parallel_pipeline: bool,
    analytical_projection: String,
}

fn phase03_diagnostics(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
) -> Phase03Diagnostics {
    let column_native = physical.column_batch_index.is_some();
    let hybrid_row_column = column_native
        && (physical.logical.filter.is_some()
            || !physical.logical.order.is_empty()
            || !physical.logical.projection.is_empty());
    let parallel_pipeline = physical.parallel_aggregate_candidate
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
        }).map_or_else(|| "none".to_string(), |(name, _)| name);

    Phase03Diagnostics {
        column_native,
        hybrid_row_column,
        vectorized_aggregate: physical.aggregate_acceleration,
        parallel_pipeline,
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
    let uses_fulltext = physical
        .operators
        .iter()
        .any(|operator| matches!(operator, crate::planner::physical::Operator::FullTextSearch));
    let uses_vector = physical
        .operators
        .iter()
        .any(|operator| matches!(operator, crate::planner::physical::Operator::VectorSearch));
    let uses_aggregate = physical
        .operators
        .iter()
        .any(|operator| matches!(operator, crate::planner::physical::Operator::Aggregate));
    let enabled =
        (uses_fulltext && uses_vector) || ((uses_fulltext || uses_vector) && uses_aggregate);
    let stages = mixed_execution_stages(
        uses_fulltext,
        uses_vector,
        uses_aggregate,
        physical.logical.filter.is_some(),
        !physical.logical.order.is_empty(),
        physical.logical.offset.is_some(),
        physical.logical.limit.is_some(),
    );

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

fn mixed_execution_stages(
    uses_fulltext: bool,
    uses_vector: bool,
    uses_aggregate: bool,
    has_filter: bool,
    has_order: bool,
    has_offset: bool,
    has_limit: bool,
) -> String {
    let mut stages = Vec::new();
    if uses_fulltext || uses_vector {
        stages.push("candidate_generation");
    }
    if has_filter {
        stages.push("metadata_prefilter");
    }
    if uses_fulltext || uses_vector {
        stages.push("exact_scoring");
    }
    if uses_aggregate {
        stages.push("analytical_grouping");
    }
    if has_order {
        stages.push("ordering");
    }
    if has_offset {
        stages.push("offset");
    }
    if has_limit {
        stages.push("limit");
    }
    if stages.is_empty() {
        "none".to_string()
    } else {
        stages.join(">")
    }
}
