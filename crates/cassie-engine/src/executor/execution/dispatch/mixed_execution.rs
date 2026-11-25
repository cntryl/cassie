//! Summarizes plans that mix scoring and analytical stages.

use super::super::plan_inspection;
use super::{LogicalPlan, SelectItem};

pub(super) fn mixed_execution_summary(plan: &LogicalPlan) -> Option<String> {
    let uses_fulltext = !plan_inspection::fulltext_query_fields(plan).is_empty();
    let uses_vector = plan_inspection::plan_uses_vector_operator(plan)
        || plan_inspection::plan_uses_function(plan, "vector_score")
        || plan_inspection::plan_uses_function(plan, "vector_distance");
    let uses_hybrid = plan_inspection::plan_uses_function(plan, "hybrid_score");
    let uses_scoring = uses_fulltext || uses_vector || uses_hybrid;
    let uses_aggregate = !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.projection.iter().any(select_item_is_aggregate);
    let mixed = (uses_fulltext && uses_vector) || (uses_aggregate && uses_scoring);
    mixed.then(|| {
        let mut stages = Vec::new();
        if uses_scoring {
            stages.push("candidate_generation");
        }
        if plan.filter.is_some() {
            stages.push("metadata_prefilter");
        }
        if uses_scoring {
            stages.push("exact_scoring");
        }
        if uses_aggregate {
            stages.push("analytical_grouping");
        }
        if !plan.order.is_empty() {
            stages.push("ordering");
        }
        if plan.offset_value().is_some() {
            stages.push("offset");
        }
        if plan.limit_value().is_some() {
            stages.push("limit");
        }
        format!("source_row_exact_baseline;stages={}", stages.join(">"))
    })
}

fn select_item_is_aggregate(item: &SelectItem) -> bool {
    matches!(
        item,
        SelectItem::Function { function, .. }
            if matches!(
                function.name.to_ascii_lowercase().as_str(),
                "count" | "sum" | "avg" | "min" | "max"
            )
    )
}
