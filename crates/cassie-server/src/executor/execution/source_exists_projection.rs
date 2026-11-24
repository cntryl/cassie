//! Admit correlated output in its existing projection and DISTINCT phase.
use super::{
    aggregate_exec, distinct_batches, ensure_query_memory_budget, plan_uses_aggregate, Batch,
    CteContext, LogicalPlan, QueryError, SourceExecutionEnv,
};

pub(super) fn apply(
    env: &SourceExecutionEnv<'_>,
    batches: Vec<Batch>,
    plan: &LogicalPlan,
    context: &CteContext,
    search: Option<&super::filter::SearchContext>,
) -> Result<Vec<Batch>, QueryError> {
    let grouped = plan_uses_aggregate(plan)
        .then(|| aggregate_exec::rewrite_aggregate_projection(&plan.projection, &plan.group_by));
    let projection = grouped.as_deref().unwrap_or(&plan.projection);
    let (memory, _) = super::super::projection_handoff::ProjectionOutputMemory::admit(
        env.controls,
        &batches,
        projection,
        false,
        true,
    )?;
    let mut batches = super::super::exists_projection::project(
        env,
        context,
        &plan.source,
        batches,
        projection,
        search,
    )?;
    let _memory = memory.retain(env.controls, &mut batches)?;
    ensure_query_memory_budget(env.controls, &batches)?;
    if plan.distinct {
        batches = distinct_batches(batches, env.controls)?;
        ensure_query_memory_budget(env.controls, &batches)?;
    }
    Ok(batches)
}
