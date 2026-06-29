use super::types::ExecutionBreakdownDurations;
use super::{Expr, QueryError, BatchRow, Cassie, CassieSession, PhysicalPlan, LogicalPlan, CteContext, HashMap, FunctionMeta, Value, QueryExecutionControls, execute_cte, source, scored, analytical_projection, index_read, ordered_read, projected_read, rollups, plan_inspection, SelectItem, QuerySource, Instant, batch};

type ExprResolution<'a> = Result<Expr, QueryError>;
type AccessPathResult = Result<Option<Vec<BatchRow>>, QueryError>;
type AccessPathFn = for<'a> fn(&mut AccessPathContext<'a>) -> AccessPathResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AccessPathRoute {
    ScalarIndex,
}

struct AccessPathExecutor {
    execute: AccessPathFn,
    records_mixed_optimization: bool,
    route: Option<AccessPathRoute>,
}

struct AccessPathContext<'a> {
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    physical: Option<&'a PhysicalPlan>,
    plan: &'a LogicalPlan,
    cte_context: &'a mut CteContext,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    controls: &'a QueryExecutionControls,
    mixed_execution: Option<String>,
}

const ACCESS_PATH_EXECUTORS: &[AccessPathExecutor] = &[
    AccessPathExecutor {
        execute: vector_distance_path,
        records_mixed_optimization: true,
        route: None,
    },
    AccessPathExecutor {
        execute: scored_search_path,
        records_mixed_optimization: true,
        route: None,
    },
    AccessPathExecutor {
        execute: analytical_projection_path,
        records_mixed_optimization: false,
        route: None,
    },
    AccessPathExecutor {
        execute: scalar_index_path,
        records_mixed_optimization: true,
        route: Some(AccessPathRoute::ScalarIndex),
    },
    AccessPathExecutor {
        execute: ordered_column_path,
        records_mixed_optimization: true,
        route: None,
    },
    AccessPathExecutor {
        execute: projected_filtered_path,
        records_mixed_optimization: true,
        route: None,
    },
    AccessPathExecutor {
        execute: rollup_path,
        records_mixed_optimization: true,
        route: None,
    },
];

pub(super) fn execute_plan(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, QueryError> {
    execute_plan_with_outer_row(
        cassie,
        session,
        plan,
        cte_context,
        user_functions,
        params,
        controls,
        None,
    )
}

pub(super) fn execute_physical_plan(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    physical: &PhysicalPlan,
    cte_context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, QueryError> {
    execute_plan_with_physical(
        cassie,
        session,
        Some(physical),
        &physical.logical,
        cte_context,
        user_functions,
        params,
        controls,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn execute_plan_with_outer_row(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
    outer_row: Option<&BatchRow>,
) -> Result<Vec<BatchRow>, QueryError> {
    execute_plan_with_physical(
        cassie,
        session,
        None,
        plan,
        cte_context,
        user_functions,
        params,
        controls,
        outer_row,
    )
}

#[allow(clippy::too_many_arguments)]
fn execute_plan_with_physical(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    physical: Option<&PhysicalPlan>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
    outer_row: Option<&BatchRow>,
) -> Result<Vec<BatchRow>, QueryError> {
    check_timeout(controls)?;
    if plan.command.is_some() {
        return Err(QueryError::General(
            "cannot execute command plans in CTE context".into(),
        ));
    }

    for cte in &plan.ctes {
        let rows = execute_cte(
            cassie,
            session,
            cte,
            cte_context,
            user_functions,
            params,
            controls,
        )?;
        cte_context.insert(cte.name.to_ascii_lowercase(), rows);
    }

    let mixed_execution = mixed_execution_summary(plan);
    if outer_row.is_none() && !source_reads_materialized_projection(cassie, &plan.source) {
        if let Some(rows) = execute_access_path_registry(
            cassie,
            session,
            physical,
            plan,
            cte_context,
            user_functions,
            params,
            controls,
            mixed_execution.clone(),
        )? {
            return Ok(rows);
        }
    }

    if let Some(reason) = mixed_execution {
        cassie
            .runtime
            .record_mixed_execution_fallback(plan.collection.clone(), reason);
    }
    source::execute_source_query_with_outer_row(
        cassie,
        session,
        plan,
        cte_context,
        user_functions,
        params,
        controls,
        outer_row,
    )
}

pub(super) fn preferred_access_path_route(
    physical: Option<&PhysicalPlan>,
) -> Option<AccessPathRoute> {
    let physical = physical?;
    physical.selected_index.as_ref()?;
    match physical.access_path {
        crate::planner::physical::ReadAccessPath::IndexSeek
        | crate::planner::physical::ReadAccessPath::PrefixScan
        | crate::planner::physical::ReadAccessPath::RangeScan
        | crate::planner::physical::ReadAccessPath::OrderedBoundedScan => {
            Some(AccessPathRoute::ScalarIndex)
        }
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_access_path_registry(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    physical: Option<&PhysicalPlan>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
    mixed_execution: Option<String>,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let mut context = AccessPathContext {
        cassie,
        session,
        physical,
        plan,
        cte_context,
        user_functions,
        params,
        controls,
        mixed_execution,
    };

    let preferred_route = preferred_access_path_route(physical);
    if let Some(route) = preferred_route {
        if let Some(rows) = execute_access_path_route(route, &mut context)? {
            record_mixed_optimization_if_needed(&context);
            return Ok(Some(rows));
        }
    }

    for executor in ACCESS_PATH_EXECUTORS {
        if preferred_route.is_some() && executor.route == preferred_route {
            continue;
        }
        if let Some(rows) = (executor.execute)(&mut context)? {
            if executor.records_mixed_optimization && context.mixed_execution.is_some() {
                record_mixed_optimization_if_needed(&context);
            }
            return Ok(Some(rows));
        }
    }

    Ok(None)
}

fn execute_access_path_route(
    route: AccessPathRoute,
    context: &mut AccessPathContext<'_>,
) -> AccessPathResult {
    match route {
        AccessPathRoute::ScalarIndex => scalar_index_path(context),
    }
}

fn record_mixed_optimization_if_needed(context: &AccessPathContext<'_>) {
    if context.mixed_execution.is_some() {
        context
            .cassie
            .runtime
            .record_mixed_execution_optimized(context.plan.collection.clone());
    }
}

fn vector_distance_path(context: &mut AccessPathContext<'_>) -> AccessPathResult {
    scored::execute_vector_distance_top_k(
        context.cassie,
        context.session,
        context.user_functions,
        context.params,
        context.plan,
    )
}

fn scored_search_path(context: &mut AccessPathContext<'_>) -> AccessPathResult {
    scored::execute_scored_search_top_k(
        context.cassie,
        context.session,
        context.user_functions,
        context.params,
        context.plan,
    )
}

fn analytical_projection_path(context: &mut AccessPathContext<'_>) -> AccessPathResult {
    analytical_projection::try_execute_analytical_projection(
        context.cassie,
        context.session,
        context.plan,
        context.cte_context,
        context.user_functions,
        context.params,
        context.controls,
    )
}

fn scalar_index_path(context: &mut AccessPathContext<'_>) -> AccessPathResult {
    index_read::execute_scalar_index_read(
        context.cassie,
        context.session,
        context.physical,
        context.plan,
        context.user_functions,
        context.params,
        context.controls,
    )
}

fn ordered_column_path(context: &mut AccessPathContext<'_>) -> AccessPathResult {
    ordered_read::execute_ordered_column_top_k(
        context.cassie,
        context.session,
        context.params,
        context.plan,
    )
}

fn projected_filtered_path(context: &mut AccessPathContext<'_>) -> AccessPathResult {
    projected_read::execute_projected_filtered_read(
        context.cassie,
        context.session,
        context.plan,
        context.user_functions,
        context.params,
        context.controls,
    )
}

fn rollup_path(context: &mut AccessPathContext<'_>) -> AccessPathResult {
    rollups::try_execute_rollup_query(
        context.cassie,
        context.plan,
        context.params,
        context.user_functions,
        context.controls,
    )
}

#[allow(clippy::nonminimal_bool)]
fn mixed_execution_summary(plan: &LogicalPlan) -> Option<String> {
    let uses_fulltext = !plan_inspection::fulltext_query_fields(plan).is_empty();
    let uses_vector = plan_inspection::plan_uses_vector_operator(plan)
        || plan_inspection::plan_uses_function(plan, "vector_score")
        || plan_inspection::plan_uses_function(plan, "vector_distance");
    let uses_hybrid = plan_inspection::plan_uses_function(plan, "hybrid_score");
    let uses_aggregate = !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.projection.iter().any(select_item_is_aggregate);
    let mixed = (uses_fulltext && uses_vector)
        || (uses_hybrid && uses_aggregate)
        || (uses_aggregate && (uses_fulltext || uses_vector || uses_hybrid));
    mixed.then(|| {
        let mut stages = Vec::new();
        if uses_fulltext || uses_vector || uses_hybrid {
            stages.push("candidate_generation");
        }
        if plan.filter.is_some() {
            stages.push("metadata_prefilter");
        }
        if uses_fulltext || uses_vector || uses_hybrid {
            stages.push("exact_scoring");
        }
        if uses_aggregate {
            stages.push("analytical_grouping");
        }
        if !plan.order.is_empty() {
            stages.push("ordering");
        }
        if plan.offset.is_some() {
            stages.push("offset");
        }
        if plan.limit.is_some() {
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

fn source_reads_materialized_projection(cassie: &Cassie, source: &QuerySource) -> bool {
    match source {
        QuerySource::Collection(name) => cassie.catalog.is_materialized_projection(name),
        QuerySource::Subquery { select, .. } => {
            source_reads_materialized_projection(cassie, &select.source)
        }
        QuerySource::Join { left, right, .. } => {
            source_reads_materialized_projection(cassie, left)
                || source_reads_materialized_projection(cassie, right)
        }
        QuerySource::Cte(_) | QuerySource::TableFunction { .. } | QuerySource::SingleRow => false,
    }
}

pub(super) fn execute_plan_with_execution_breakdown(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<(Vec<BatchRow>, ExecutionBreakdownDurations), QueryError> {
    if let Some(output) = projected_read::execute_projected_filtered_read_with_breakdown(
        cassie,
        session,
        plan,
        user_functions,
        params,
        controls,
    )? {
        return Ok(output);
    }

    let started = Instant::now();
    let rows = execute_plan(
        cassie,
        session,
        plan,
        cte_context,
        user_functions,
        params,
        controls,
    )?;
    let breakdown = ExecutionBreakdownDurations {
        scan: started.elapsed(),
        ..ExecutionBreakdownDurations::default()
    };
    Ok((rows, breakdown))
}

pub(super) fn resolve_exists_expr<'a>(
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    expr: &'a Expr,
    cte_context: &'a CteContext,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    controls: &'a QueryExecutionControls,
) -> ExprResolution<'a> {
    match expr {
        Expr::Binary { left, op, right } => resolve_binary_exists_expr(
            cassie, session, left, op, right, cte_context, user_functions, params, controls,
        ),
        Expr::IsNull { expr, negated } => resolve_is_null_exists_expr(
            cassie, session, expr, *negated, cte_context, user_functions, params, controls,
        ),
        Expr::InList { expr, values, negated } => resolve_in_list_exists_expr(
            cassie, session, expr, values, *negated, cte_context, user_functions, params, controls,
        ),
        Expr::Between {
            expr,
            low,
            high,
            negated,
        } => resolve_between_exists_expr(
            cassie, session, expr, low, high, *negated, cte_context, user_functions, params,
            controls,
        ),
        Expr::Not { expr } => resolve_not_exists_expr(
            cassie, session, expr, cte_context, user_functions, params, controls,
        ),
        Expr::Cast { expr, data_type } => resolve_cast_exists_expr(
            cassie, session, expr, data_type, cte_context, user_functions, params, controls,
        ),
        Expr::Exists(statement) => {
            let logical = build_logical_plan(statement.as_ref())?;
            let logical = bounded_exists_logical(logical);
            let mut subquery_context = cte_context.clone();
            let rows = execute_plan(
                cassie,
                session,
                &logical,
                &mut subquery_context,
                user_functions,
                params,
                controls,
            )?;
            Ok(Expr::BoolLiteral(!rows.is_empty()))
        }
        Expr::Column(_)
        | Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::Function(_) => Ok(expr.clone()),
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_binary_exists_expr<'a>(
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    left: &'a Expr,
    op: &'a crate::sql::ast::BinaryOp,
    right: &'a Expr,
    cte_context: &'a CteContext,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    controls: &'a QueryExecutionControls,
) -> ExprResolution<'a> {
    Ok(Expr::Binary {
        left: Box::new(resolve_exists_expr(
            cassie,
            session,
            left,
            cte_context,
            user_functions,
            params,
            controls,
        )?),
        op: op.clone(),
        right: Box::new(resolve_exists_expr(
            cassie,
            session,
            right,
            cte_context,
            user_functions,
            params,
            controls,
        )?),
    })
}

#[allow(clippy::too_many_arguments)]
fn resolve_is_null_exists_expr<'a>(
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    expr: &'a Expr,
    negated: bool,
    cte_context: &'a CteContext,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    controls: &'a QueryExecutionControls,
) -> ExprResolution<'a> {
    Ok(Expr::IsNull {
        expr: Box::new(resolve_exists_expr(
            cassie,
            session,
            expr,
            cte_context,
            user_functions,
            params,
            controls,
        )?),
        negated,
    })
}

#[allow(clippy::too_many_arguments)]
fn resolve_in_list_exists_expr<'a>(
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    expr: &'a Expr,
    values: &'a [Expr],
    negated: bool,
    cte_context: &'a CteContext,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    controls: &'a QueryExecutionControls,
) -> ExprResolution<'a> {
    let expr = resolve_exists_expr(
        cassie,
        session,
        expr,
        cte_context,
        user_functions,
        params,
        controls,
    )?;
    let mut resolved_values = Vec::with_capacity(values.len());
    for value in values {
        resolved_values.push(resolve_exists_expr(
            cassie,
            session,
            value,
            cte_context,
            user_functions,
            params,
            controls,
        )?);
    }
    Ok(Expr::InList {
        expr: Box::new(expr),
        values: resolved_values,
        negated,
    })
}

#[allow(clippy::too_many_arguments)]
fn resolve_between_exists_expr<'a>(
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    expr: &'a Expr,
    low: &'a Expr,
    high: &'a Expr,
    negated: bool,
    cte_context: &'a CteContext,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    controls: &'a QueryExecutionControls,
) -> ExprResolution<'a> {
    Ok(Expr::Between {
        expr: Box::new(resolve_exists_expr(
            cassie,
            session,
            expr,
            cte_context,
            user_functions,
            params,
            controls,
        )?),
        low: Box::new(resolve_exists_expr(
            cassie,
            session,
            low,
            cte_context,
            user_functions,
            params,
            controls,
        )?),
        high: Box::new(resolve_exists_expr(
            cassie,
            session,
            high,
            cte_context,
            user_functions,
            params,
            controls,
        )?),
        negated,
    })
}

#[allow(clippy::too_many_arguments)]
fn resolve_not_exists_expr<'a>(
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    expr: &'a Expr,
    cte_context: &'a CteContext,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    controls: &'a QueryExecutionControls,
) -> ExprResolution<'a> {
    Ok(Expr::Not {
        expr: Box::new(resolve_exists_expr(
            cassie,
            session,
            expr,
            cte_context,
            user_functions,
            params,
            controls,
        )?),
    })
}

#[allow(clippy::too_many_arguments)]
fn resolve_cast_exists_expr<'a>(
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    expr: &'a Expr,
    data_type: &'a crate::types::DataType,
    cte_context: &'a CteContext,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    controls: &'a QueryExecutionControls,
) -> ExprResolution<'a> {
    Ok(Expr::Cast {
        expr: Box::new(resolve_exists_expr(
            cassie,
            session,
            expr,
            cte_context,
            user_functions,
            params,
            controls,
        )?),
        data_type: data_type.clone(),
    })
}

pub(super) fn build_logical_plan(
    statement: &crate::sql::ast::ParsedStatement,
) -> Result<LogicalPlan, QueryError> {
    let plan = crate::planner::logical::plan(&crate::sql::binder::BoundStatement {
        statement: statement.clone(),
        indexes: Vec::new(),
    })
    .map_err(|error| QueryError::General(error.to_string()))?;

    if plan.command.is_some() {
        return Err(QueryError::General(
            "CTE statements cannot include command statements".into(),
        ));
    }

    Ok(plan)
}

fn bounded_exists_logical(mut logical: LogicalPlan) -> LogicalPlan {
    logical.limit = Some(
        logical
            .limit
            .map_or(1, |limit| i64::from(limit > 0)),
    );
    logical
}

pub(super) fn check_timeout(controls: &QueryExecutionControls) -> Result<(), QueryError> {
    if controls.is_timed_out() {
        return Err(QueryError::General("query timeout exceeded".to_string()));
    }

    Ok(())
}

pub(super) fn ensure_temp_budget(
    controls: &QueryExecutionControls,
    batches: &[batch::Batch],
) -> Result<(), QueryError> {
    let bytes = estimate_batch_bytes(batches);
    if bytes > controls.temp_spill_budget_bytes {
        return Err(QueryError::General(format!(
            "temporary storage budget exceeded: {bytes} > {}",
            controls.temp_spill_budget_bytes
        )));
    }

    Ok(())
}

pub(super) fn ensure_temp_budget_for_rows(
    controls: &QueryExecutionControls,
    rows: &[Vec<(String, Value)>],
) -> Result<(), QueryError> {
    let bytes = rows
        .iter()
        .map(|row| {
            serde_json::to_vec(row)
                .map(|bytes| bytes.len())
                .unwrap_or_default()
        })
        .sum::<usize>();

    if bytes > controls.temp_spill_budget_bytes {
        return Err(QueryError::General(format!(
            "temporary storage budget exceeded: {bytes} > {}",
            controls.temp_spill_budget_bytes
        )));
    }

    Ok(())
}

fn estimate_batch_bytes(batches: &[batch::Batch]) -> usize {
    batches
        .iter()
        .flat_map(|batch| batch.iter())
        .map(|row| {
            serde_json::to_vec(row.entries())
                .map(|bytes| bytes.len())
                .unwrap_or_default()
        })
        .sum()
}
