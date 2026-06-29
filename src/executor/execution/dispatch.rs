use super::types::ExecutionBreakdownDurations;
use super::*;

type ExprResolution<'a> = Result<Expr, QueryError>;
type AccessPathResult = Result<Option<Vec<BatchRow>>, QueryError>;
type AccessPathFn = for<'a> fn(&mut AccessPathContext<'a>) -> AccessPathResult;

struct AccessPathExecutor {
    execute: AccessPathFn,
    records_mixed_optimization: bool,
}

struct AccessPathContext<'a> {
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
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
    },
    AccessPathExecutor {
        execute: scored_search_path,
        records_mixed_optimization: true,
    },
    AccessPathExecutor {
        execute: analytical_projection_path,
        records_mixed_optimization: false,
    },
    AccessPathExecutor {
        execute: scalar_index_path,
        records_mixed_optimization: true,
    },
    AccessPathExecutor {
        execute: ordered_column_path,
        records_mixed_optimization: true,
    },
    AccessPathExecutor {
        execute: projected_filtered_path,
        records_mixed_optimization: true,
    },
    AccessPathExecutor {
        execute: rollup_path,
        records_mixed_optimization: true,
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

#[allow(clippy::too_many_arguments)]
fn execute_access_path_registry(
    cassie: &Cassie,
    session: Option<&CassieSession>,
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
        plan,
        cte_context,
        user_functions,
        params,
        controls,
        mixed_execution,
    };

    for executor in ACCESS_PATH_EXECUTORS {
        if let Some(rows) = (executor.execute)(&mut context)? {
            if executor.records_mixed_optimization && context.mixed_execution.is_some() {
                context
                    .cassie
                    .runtime
                    .record_mixed_execution_optimized(context.plan.collection.clone());
            }
            return Ok(Some(rows));
        }
    }

    Ok(None)
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
        Expr::Binary { left, op, right } => Ok(Expr::Binary {
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
        }),
        Expr::IsNull { expr, negated } => Ok(Expr::IsNull {
            expr: Box::new(resolve_exists_expr(
                cassie,
                session,
                expr,
                cte_context,
                user_functions,
                params,
                controls,
            )?),
            negated: *negated,
        }),
        Expr::InList {
            expr,
            values,
            negated,
        } => {
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
                negated: *negated,
            })
        }
        Expr::Between {
            expr,
            low,
            high,
            negated,
        } => Ok(Expr::Between {
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
            negated: *negated,
        }),
        Expr::Not { expr } => Ok(Expr::Not {
            expr: Box::new(resolve_exists_expr(
                cassie,
                session,
                expr,
                cte_context,
                user_functions,
                params,
                controls,
            )?),
        }),
        Expr::Cast { expr, data_type } => Ok(Expr::Cast {
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
        }),
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
            .map(|limit| if limit <= 0 { 0 } else { 1 })
            .unwrap_or(1),
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
