//! Blocking phases borrow the captured statement context and actual input rows.
use super::{
    aggregate_exec, distinct_on_batches, ensure_query_memory_budget, filter, plan_uses_aggregate,
    sort, window_exec, Batch, BatchRow, CteContext, Expr, LogicalPlan, PhaseExecutionEnv,
    QueryError, SourceExecutionEnv,
};

pub(super) fn window(
    env: &SourceExecutionEnv<'_>,
    ctes: &CteContext,
    batches: Vec<Batch>,
    plan: &LogicalPlan,
    search: Option<&filter::SearchContext>,
) -> Result<Vec<Batch>, QueryError> {
    let grouped_windows = grouped_window_inputs(plan, env.controls)?;
    let projection = grouped_windows
        .as_ref()
        .map_or(plan.projection.as_slice(), |(windows, _)| {
            windows.as_slice()
        });
    let deferred = projection.iter().any(|item| match item {
        crate::sql::ast::SelectItem::WindowFunction { function, .. } => function
            .args
            .iter()
            .chain(function.filter.as_deref())
            .chain(&function.partition_by)
            .chain(function.order_by.iter().map(|order| &order.expr))
            .any(|expr| {
                super::super::exists_correlated::contains_exists_or_routine_subquery(
                    expr,
                    env.user_functions,
                )
            }),
        _ => false,
    });
    let batches = if deferred {
        window_exec::apply_resolving(env, ctes, &plan.source, batches, projection, search)?
    } else {
        window_exec::apply_window_functions(
            batches,
            projection,
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
        let grouped_keys = if plan_uses_aggregate(plan)
            && plan
                .distinct_on
                .iter()
                .any(aggregate_exec::contains_aggregate)
        {
            let mut memory = env.controls.reserve_query_memory(0)?;
            let mut keys = Vec::new();
            for expr in &plan.distinct_on {
                memory.try_grow(crate::sql::parse_analysis::expression_clone_bytes(
                    expr,
                    0,
                    env.controls,
                )?)?;
                crate::runtime::owned_memory::grow_vec(&mut keys, 1, &mut memory)?;
                keys.push(aggregate_exec::rewrite_aggregate_expr(expr, &plan.group_by));
            }
            Some((keys, memory))
        } else {
            None
        };
        let keys = grouped_keys
            .as_ref()
            .map_or(plan.distinct_on.as_slice(), |(keys, _)| keys.as_slice());
        batches = distinct_on_batches(
            batches,
            keys,
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

impl PhaseExecutionEnv<'_> {
    pub(super) fn aggregate_context<'a>(
        &'a self,
        plan: &'a LogicalPlan,
    ) -> aggregate_exec::AggregateExecutionContext<'a> {
        aggregate_exec::AggregateExecutionContext {
            plan,
            params: self.params,
            parameter_type_oids: self.parameter_type_oids,
            search_context: self.search_context,
            user_functions: self.user_functions,
            session: self.session,
            controls: self.controls,
            #[cfg(test)]
            after_partition_row: None,
        }
    }
}

pub(super) fn prepare_subtotal(
    env: &SourceExecutionEnv<'_>,
    phase: &PhaseExecutionEnv<'_>,
    ctes: &CteContext,
    plan: &LogicalPlan,
    batches: &mut [Batch],
) -> Result<Option<aggregate_exec::subtotal::PreparedPlan>, QueryError> {
    plan.grouping_subtotal
        .then(|| {
            aggregate_exec::subtotal::prepare_plan(
                batches,
                &phase.aggregate_context(plan),
                &|row, expr, memory| {
                    super::super::exists_phase::evaluate_with_owner(
                        env,
                        ctes,
                        &plan.source,
                        row,
                        expr,
                        phase.search_context,
                        memory,
                    )
                },
            )
        })
        .transpose()
}

type GroupedWindows = (
    Vec<crate::sql::ast::SelectItem>,
    crate::runtime::QueryMemoryReservation,
);

fn grouped_window_inputs(
    plan: &LogicalPlan,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<Option<GroupedWindows>, QueryError> {
    use crate::sql::ast::SelectItem;
    if plan.group_by.is_empty() || !plan.projection.iter().any(|item| matches!(item, SelectItem::WindowFunction {function,..} if function.args.iter().chain(function.filter.as_deref()).chain(&function.partition_by).chain(function.order_by.iter().map(|key| &key.expr)).any(aggregate_exec::contains_aggregate))) {
        return Ok(None);
    }
    let mut memory = controls.reserve_query_memory(0)?;
    let mut windows = Vec::new();
    for item in &plan.projection {
        if !matches!(item, SelectItem::WindowFunction { .. }) {
            continue;
        }
        memory.try_grow(crate::executor::retained_memory::mul(
            crate::sql::parse_analysis::item_clone_bytes(item, 0, controls)?,
            2,
        )?)?;
        crate::runtime::owned_memory::grow_vec(&mut windows, 1, &mut memory)?;
        let mut window = item.clone();
        if let SelectItem::WindowFunction { function, .. } = &mut window {
            for expr in function
                .args
                .iter_mut()
                .chain(function.filter.as_deref_mut())
                .chain(&mut function.partition_by)
                .chain(function.order_by.iter_mut().map(|key| &mut key.expr))
            {
                *expr = aggregate_exec::rewrite_aggregate_expr(expr, &plan.group_by);
            }
        }
        windows.push(window);
    }
    Ok(Some((windows, memory)))
}
