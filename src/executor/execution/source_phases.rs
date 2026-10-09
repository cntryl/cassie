//! Blocking phases borrow the captured statement context and actual input rows.
use super::{
    aggregate_exec, distinct_on_batches, ensure_query_memory_budget, filter, plan_uses_aggregate,
    sort, window_exec, Batch, BatchRow, CteContext, Expr, LogicalPlan, QueryError,
    SourceExecutionEnv,
};

pub(super) fn window(
    env: &SourceExecutionEnv<'_>,
    ctes: &CteContext,
    batches: Vec<Batch>,
    plan: &LogicalPlan,
    search: Option<&filter::SearchContext>,
) -> Result<Vec<Batch>, QueryError> {
    let deferred = plan.projection.iter().any(|item| match item {
        crate::sql::ast::SelectItem::WindowFunction { function, .. } => function
            .args
            .iter()
            .any(super::super::exists_correlated::contains_exists),
        _ => false,
    });
    let batches = if deferred {
        window_exec::apply_resolving(env, ctes, &plan.source, batches, &plan.projection, search)?
    } else {
        window_exec::apply_window_functions(
            batches,
            &plan.projection,
            env.params,
            search,
            env.user_functions,
            env.session,
            env.controls,
        )?
    };
    ensure_query_memory_budget(env.controls, &batches)?;
    Ok(batches)
}

pub(super) fn sort(
    env: &SourceExecutionEnv<'_>,
    ctes: &CteContext,
    mut batches: Vec<Batch>,
    plan: &LogicalPlan,
    search: Option<&filter::SearchContext>,
) -> Result<Vec<Batch>, QueryError> {
    let grouped_order = plan_uses_aggregate(plan).then(|| {
        aggregate_exec::rewrite_aggregate_order(&plan.order, &plan.projection, &plan.group_by)
    });
    let (order, projection) = match &grouped_order {
        Some(order) => (order.as_slice(), &[][..]),
        None => (plan.order.as_slice(), plan.projection.as_slice()),
    };
    let eval = sort::EvalInput {
        order,
        projection,
        params: env.params,
        search_context: search,
        user_functions: env.user_functions,
        session: env.session,
    };
    if !plan.distinct_on.is_empty() || (plan.set.is_none() && !plan.order.is_empty()) {
        let deferred = sort::requires_resolver(&eval);
        batches = if deferred {
            let evaluate = |row: &BatchRow, expr: &Expr| {
                super::super::exists_phase::evaluate(env, ctes, &plan.source, row, expr, search)
            };
            sort::sort_batches_resolving(batches, &eval, env.controls, &evaluate)?
        } else {
            sort::sort_batches_with_controls(batches, &eval, env.controls)?
        };
        ensure_query_memory_budget(env.controls, &batches)?;
    }
    if !plan.distinct_on.is_empty() {
        batches = distinct_on_batches(
            batches,
            &plan.distinct_on,
            env.params,
            search,
            env.user_functions,
            env.session,
            env.controls,
        )?;
        ensure_query_memory_budget(env.controls, &batches)?;
    }
    Ok(batches)
}
