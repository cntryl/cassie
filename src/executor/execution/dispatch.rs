use super::types::ExecutionBreakdownDurations;
use super::{
    analytical_projection, batch, execute_cte, index_read, ordered_read, plan_inspection,
    projected_read, rollups, scored, source, BatchRow, Cassie, CassieSession, CteContext, Expr,
    FunctionMeta, HashMap, Instant, LogicalPlan, PhysicalPlan, QueryError, QueryExecutionControls,
    QuerySource, SelectItem, Value,
};

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

pub(super) struct PlanExecutionEnv<'a> {
    pub(super) cassie: &'a Cassie,
    pub(super) session: Option<&'a CassieSession>,
    pub(super) user_functions: &'a HashMap<String, FunctionMeta>,
    pub(super) params: &'a [Value],
    pub(super) controls: &'a QueryExecutionControls,
}

struct AccessPathContext<'a> {
    env: &'a PlanExecutionEnv<'a>,
    physical: Option<&'a PhysicalPlan>,
    plan: &'a LogicalPlan,
    cte_context: &'a mut CteContext,
    mixed_execution: Option<String>,
}

pub(super) struct ExistsResolutionContext<'a> {
    pub(super) cassie: &'a Cassie,
    pub(super) session: Option<&'a CassieSession>,
    pub(super) cte_context: &'a CteContext,
    pub(super) user_functions: &'a HashMap<String, FunctionMeta>,
    pub(super) params: &'a [Value],
    pub(super) controls: &'a QueryExecutionControls,
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
    let env = plan_execution_env(cassie, session, user_functions, params, controls);
    execute_plan_with_outer_row(&env, plan, cte_context, None)
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
    let env = plan_execution_env(cassie, session, user_functions, params, controls);
    execute_plan_with_physical(&env, Some(physical), &physical.logical, cte_context, None)
}

pub(super) fn plan_execution_env<'a>(
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    controls: &'a QueryExecutionControls,
) -> PlanExecutionEnv<'a> {
    PlanExecutionEnv {
        cassie,
        session,
        user_functions,
        params,
        controls,
    }
}

pub(super) fn execute_plan_with_outer_row(
    env: &PlanExecutionEnv<'_>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    outer_row: Option<&BatchRow>,
) -> Result<Vec<BatchRow>, QueryError> {
    execute_plan_with_physical(env, None, plan, cte_context, outer_row)
}

fn execute_plan_with_physical(
    env: &PlanExecutionEnv<'_>,
    physical: Option<&PhysicalPlan>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    outer_row: Option<&BatchRow>,
) -> Result<Vec<BatchRow>, QueryError> {
    check_timeout(env.controls)?;
    if plan.command.is_some() {
        return Err(QueryError::General(
            "cannot execute command plans in CTE context".into(),
        ));
    }

    for cte in &plan.ctes {
        let rows = execute_cte(
            env.cassie,
            env.session,
            cte,
            cte_context,
            env.user_functions,
            env.params,
            env.controls,
        )?;
        cte_context.insert(cte.name.to_ascii_lowercase(), rows);
    }

    let mixed_execution = mixed_execution_summary(plan);
    if outer_row.is_none() && !source_reads_materialized_projection(env.cassie, &plan.source) {
        let mut access_path_context = AccessPathContext {
            env,
            physical,
            plan,
            cte_context,
            mixed_execution: mixed_execution.clone(),
        };
        if let Some(rows) = execute_access_path_registry(&mut access_path_context)? {
            return Ok(rows);
        }
    }

    if let Some(reason) = mixed_execution {
        env.cassie
            .runtime
            .record_mixed_execution_fallback(plan.collection.clone(), reason);
    }
    let source_env = source::source_execution_env(
        env.cassie,
        env.session,
        env.user_functions,
        env.params,
        env.controls,
    );
    source::execute_source_query_with_outer_row(&source_env, plan, cte_context, outer_row)
}

pub(super) fn preferred_access_path_route(
    physical: Option<&PhysicalPlan>,
) -> Option<AccessPathRoute> {
    let physical = physical?;
    physical.read.selected_index.as_ref()?;
    match physical.read.access_path {
        crate::planner::physical::ReadAccessPath::IndexSeek
        | crate::planner::physical::ReadAccessPath::PrefixScan
        | crate::planner::physical::ReadAccessPath::RangeScan
        | crate::planner::physical::ReadAccessPath::OrderedBoundedScan => {
            Some(AccessPathRoute::ScalarIndex)
        }
        _ => None,
    }
}

fn execute_access_path_registry(
    context: &mut AccessPathContext<'_>,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let preferred_route = preferred_access_path_route(context.physical);
    if let Some(route) = preferred_route {
        if let Some(rows) = execute_access_path_route(route, context)? {
            record_mixed_optimization_if_needed(context);
            return Ok(Some(rows));
        }
    }

    for executor in ACCESS_PATH_EXECUTORS {
        if preferred_route.is_some() && executor.route == preferred_route {
            continue;
        }
        if let Some(rows) = (executor.execute)(context)? {
            if executor.records_mixed_optimization && context.mixed_execution.is_some() {
                record_mixed_optimization_if_needed(context);
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
            .env
            .cassie
            .runtime
            .record_mixed_execution_optimized(context.plan.collection.clone());
    }
}

fn vector_distance_path(context: &mut AccessPathContext<'_>) -> AccessPathResult {
    scored::execute_vector_distance_top_k(
        context.env.cassie,
        context.env.session,
        context.env.user_functions,
        context.env.params,
        context.plan,
        context.env.controls,
    )
}

fn scored_search_path(context: &mut AccessPathContext<'_>) -> AccessPathResult {
    scored::execute_scored_search_top_k(
        context.env.cassie,
        context.env.session,
        context.env.user_functions,
        context.env.params,
        context.plan,
        context.env.controls,
    )
}

fn analytical_projection_path(context: &mut AccessPathContext<'_>) -> AccessPathResult {
    analytical_projection::try_execute_analytical_projection(
        context.env.cassie,
        context.env.session,
        context.plan,
        context.cte_context,
        context.env.user_functions,
        context.env.params,
        context.env.controls,
    )
}

fn scalar_index_path(context: &mut AccessPathContext<'_>) -> AccessPathResult {
    index_read::execute_scalar_index_read(
        context.env.cassie,
        context.env.session,
        context.physical,
        context.plan,
        context.env.user_functions,
        context.env.params,
        context.env.controls,
    )
}

fn ordered_column_path(context: &mut AccessPathContext<'_>) -> AccessPathResult {
    ordered_read::execute_ordered_column_top_k(
        context.env.cassie,
        context.env.session,
        context.env.params,
        context.plan,
    )
}

fn projected_filtered_path(context: &mut AccessPathContext<'_>) -> AccessPathResult {
    projected_read::execute_projected_filtered_read(
        context.env.cassie,
        context.env.session,
        context.plan,
        context.env.user_functions,
        context.env.params,
        context.env.controls,
    )
}

fn rollup_path(context: &mut AccessPathContext<'_>) -> AccessPathResult {
    rollups::try_execute_rollup_query(
        context.env.cassie,
        context.plan,
        context.env.params,
        context.env.user_functions,
        context.env.controls,
    )
}

fn mixed_execution_summary(plan: &LogicalPlan) -> Option<String> {
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
    context: &'a ExistsResolutionContext<'a>,
    expr: &'a Expr,
) -> ExprResolution<'a> {
    match expr {
        Expr::Binary { left, op, right } => resolve_binary_exists_expr(context, left, op, right),
        Expr::IsNull { expr, negated } => resolve_is_null_exists_expr(context, expr, *negated),
        Expr::InList {
            expr,
            values,
            negated,
        } => resolve_in_list_exists_expr(context, expr, values, *negated),
        Expr::Between {
            expr,
            low,
            high,
            negated,
        } => resolve_between_exists_expr(context, expr, low, high, *negated),
        Expr::Not { expr } => resolve_not_exists_expr(context, expr),
        Expr::Cast { expr, data_type } => resolve_cast_exists_expr(context, expr, data_type),
        Expr::Exists(statement) => {
            let logical = build_exists_logical_plan(context, statement.as_ref())?;
            let logical = bounded_exists_logical(logical);
            let mut subquery_context = context.cte_context.clone();
            let env = plan_execution_env(
                context.cassie,
                context.session,
                context.user_functions,
                context.params,
                context.controls,
            );
            let rows = execute_plan_with_outer_row(&env, &logical, &mut subquery_context, None)?;
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

fn resolve_binary_exists_expr<'a>(
    context: &'a ExistsResolutionContext<'a>,
    left: &'a Expr,
    op: &'a crate::sql::ast::BinaryOp,
    right: &'a Expr,
) -> ExprResolution<'a> {
    Ok(Expr::Binary {
        left: Box::new(resolve_exists_expr(context, left)?),
        op: op.clone(),
        right: Box::new(resolve_exists_expr(context, right)?),
    })
}

fn resolve_is_null_exists_expr<'a>(
    context: &'a ExistsResolutionContext<'a>,
    expr: &'a Expr,
    negated: bool,
) -> ExprResolution<'a> {
    Ok(Expr::IsNull {
        expr: Box::new(resolve_exists_expr(context, expr)?),
        negated,
    })
}

fn resolve_in_list_exists_expr<'a>(
    context: &'a ExistsResolutionContext<'a>,
    expr: &'a Expr,
    values: &'a [Expr],
    negated: bool,
) -> ExprResolution<'a> {
    let expr = resolve_exists_expr(context, expr)?;
    let mut resolved_values = Vec::with_capacity(values.len());
    for value in values {
        resolved_values.push(resolve_exists_expr(context, value)?);
    }
    Ok(Expr::InList {
        expr: Box::new(expr),
        values: resolved_values,
        negated,
    })
}

fn resolve_between_exists_expr<'a>(
    context: &'a ExistsResolutionContext<'a>,
    expr: &'a Expr,
    low: &'a Expr,
    high: &'a Expr,
    negated: bool,
) -> ExprResolution<'a> {
    Ok(Expr::Between {
        expr: Box::new(resolve_exists_expr(context, expr)?),
        low: Box::new(resolve_exists_expr(context, low)?),
        high: Box::new(resolve_exists_expr(context, high)?),
        negated,
    })
}

fn resolve_not_exists_expr<'a>(
    context: &'a ExistsResolutionContext<'a>,
    expr: &'a Expr,
) -> ExprResolution<'a> {
    Ok(Expr::Not {
        expr: Box::new(resolve_exists_expr(context, expr)?),
    })
}

fn resolve_cast_exists_expr<'a>(
    context: &'a ExistsResolutionContext<'a>,
    expr: &'a Expr,
    data_type: &'a crate::types::DataType,
) -> ExprResolution<'a> {
    Ok(Expr::Cast {
        expr: Box::new(resolve_exists_expr(context, expr)?),
        data_type: data_type.clone(),
    })
}

fn build_exists_logical_plan(
    context: &ExistsResolutionContext<'_>,
    statement: &crate::sql::ast::ParsedStatement,
) -> Result<LogicalPlan, QueryError> {
    let binding_context = exists_binding_context(context);
    let bound = crate::sql::binder::bind_with_context(
        statement.clone(),
        &context.cassie.catalog,
        &binding_context,
    )
    .map_err(|error| QueryError::General(error.to_string()))?;
    let plan = crate::planner::logical::plan(&bound)
        .map_err(|error| QueryError::General(error.to_string()))?;
    if plan.command.is_some() {
        return Err(QueryError::General(
            "CTE statements cannot include command statements".into(),
        ));
    }
    Ok(plan)
}

fn statement_binding_context(
    cassie: &Cassie,
    session: Option<&CassieSession>,
) -> crate::sql::binder::BindingContext {
    let database = session
        .and_then(CassieSession::current_database)
        .unwrap_or(cassie.default_database.as_str())
        .to_string();
    let search_path = session.map_or_else(
        || vec![crate::catalog::DEFAULT_SCHEMA.to_string()],
        CassieSession::search_path,
    );
    if cassie.database_catalog_enforced() {
        crate::sql::binder::BindingContext::scoped(database, search_path)
    } else {
        crate::sql::binder::BindingContext::unscoped(database, search_path)
    }
}

fn exists_binding_context(
    context: &ExistsResolutionContext<'_>,
) -> crate::sql::binder::BindingContext {
    statement_binding_context(context.cassie, context.session)
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

pub(super) fn build_logical_plan_in_session(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::ParsedStatement,
) -> Result<LogicalPlan, QueryError> {
    let context = statement_binding_context(cassie, session);
    let bound = crate::sql::binder::bind_with_context(statement.clone(), &cassie.catalog, &context)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let plan = crate::planner::logical::plan(&bound)
        .map_err(|error| QueryError::General(error.to_string()))?;

    if plan.command.is_some() {
        return Err(QueryError::General(
            "CTE statements cannot include command statements".into(),
        ));
    }

    Ok(plan)
}

fn bounded_exists_logical(mut logical: LogicalPlan) -> LogicalPlan {
    logical.limit = Some(logical.limit.map_or(1, |limit| i64::from(limit > 0)));
    logical
}

pub(super) fn check_timeout(controls: &QueryExecutionControls) -> Result<(), QueryError> {
    if controls.is_cancelled() {
        return Err(QueryError::General("query canceled".to_string()));
    }
    if controls.is_timed_out() {
        return Err(QueryError::General("query timeout exceeded".to_string()));
    }

    Ok(())
}

pub(super) fn ensure_query_memory_budget(
    controls: &QueryExecutionControls,
    batches: &[batch::Batch],
) -> Result<crate::runtime::QueryMemoryReservation, QueryError> {
    let bytes = estimate_batch_bytes(batches);
    controls
        .reserve_query_memory(bytes)
        .map_err(QueryError::from)
}

pub(super) fn ensure_query_memory_budget_for_rows(
    controls: &QueryExecutionControls,
    rows: &[Vec<(String, Value)>],
) -> Result<crate::runtime::QueryMemoryReservation, QueryError> {
    let bytes = rows
        .iter()
        .map(|row| {
            serde_json::to_vec(row)
                .map(|bytes| bytes.len())
                .unwrap_or_default()
        })
        .sum::<usize>();

    controls
        .reserve_query_memory(bytes)
        .map_err(QueryError::from)
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
