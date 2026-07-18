use super::plan_inspection;
use super::{
    aggregate, aggregate_accel, aggregate_exec, batch, build_logical_plan_in_session, catalog,
    check_timeout, deduce_text_fields, ensure_query_memory_budget, execute_plan,
    execute_plan_with_outer_row, filter, graph, load_fulltext_index_options, plan_execution_env,
    projection, resolve_exists_expr, row_signature, scan, sort, virtual_views, window_exec,
    AnalyzerConfig, Batch, BatchRow, BinaryOp, Cassie, CassieSession, CteContext,
    ExistsResolutionContext, Expr, FunctionMeta, HashMap, HashSet, Instant, JoinKind, LogicalPlan,
    QueryError, QueryExecutionControls, QuerySource, Value,
};

#[path = "source_join.rs"]
mod source_join;

type SourceExecution = Result<(Vec<Batch>, Vec<String>), QueryError>;

pub(super) struct SourceExecutionEnv<'a> {
    pub(super) cassie: &'a Cassie,
    pub(super) session: Option<&'a CassieSession>,
    pub(super) user_functions: &'a HashMap<String, FunctionMeta>,
    pub(super) params: &'a [Value],
    pub(super) controls: &'a QueryExecutionControls,
}

pub(super) fn execute_query_source(
    env: &SourceExecutionEnv<'_>,
    source: &QuerySource,
    cte_context: &mut CteContext,
    qualify: bool,
    outer_row: Option<&BatchRow>,
    row_budget: Option<usize>,
) -> SourceExecution {
    match source {
        QuerySource::Collection(name) => execute_collection_source(env, name, qualify, row_budget),
        QuerySource::SingleRow => execute_single_row_source(env),
        QuerySource::TableFunction {
            name,
            function,
            lateral,
        } => execute_table_function_source(env, name, function, *lateral, outer_row, qualify),
        QuerySource::Cte(name) => execute_cte_source(env, cte_context, name, qualify),
        QuerySource::Subquery {
            alias,
            select,
            lateral,
        } => execute_subquery_source(env, cte_context, alias, select, *lateral, outer_row),
        QuerySource::Join {
            left,
            right,
            kind,
            on,
        } => {
            let (batches, text_fields) = source_join::execute_join_source(
                env,
                source_join::JoinExecutionSpec {
                    left,
                    right,
                    kind: *kind,
                    on,
                    outer_row,
                    row_budget,
                },
                cte_context,
            )?;
            ensure_query_memory_budget(env.controls, &batches)?;
            Ok((batches, text_fields))
        }
    }
}

fn execute_collection_source(
    env: &SourceExecutionEnv<'_>,
    name: &str,
    qualify: bool,
    row_budget: Option<usize>,
) -> SourceExecution {
    if let Some(rows) = virtual_views::rows(&env.cassie.catalog, name, env.session) {
        return finalize_source_batches(
            env,
            materialize_virtual_rows(rows),
            Vec::new(),
            qualify,
            name,
        );
    }

    if let Some(view) = env.cassie.catalog.get_view(name) {
        return execute_view_source(env, name, &view, qualify);
    }

    if let Some(projection) = env.cassie.catalog.get_materialized_projection(name) {
        return execute_materialized_projection_source(env, name, &projection, qualify, row_budget);
    }

    let batches = scan::scan_limit(env.cassie, env.session, name, row_budget, env.controls)?;
    finalize_source_batches(
        env,
        batches,
        env.cassie.catalog.text_fields(name),
        qualify,
        name,
    )
}

fn execute_view_source(
    env: &SourceExecutionEnv<'_>,
    name: &str,
    view: &crate::catalog::ViewMeta,
    qualify: bool,
) -> SourceExecution {
    let parsed = crate::sql::parser::parse_statement(&view.query)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let logical = build_logical_plan_in_session(env.cassie, env.session, &parsed)?;
    let mut view_cte_context = CteContext::new();
    let rows = execute_plan(
        env.cassie,
        env.session,
        &logical,
        &mut view_cte_context,
        env.user_functions,
        env.params,
        env.controls,
    )?;
    let rows = project_rows_to_schema(rows, &view.schema, name)?;
    let text_fields = schema_text_fields(&view.schema);
    finalize_source_batches(
        env,
        batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE),
        text_fields,
        qualify,
        name,
    )
}

fn execute_materialized_projection_source(
    env: &SourceExecutionEnv<'_>,
    name: &str,
    projection: &crate::catalog::ProjectionMeta,
    qualify: bool,
    row_budget: Option<usize>,
) -> SourceExecution {
    let output_collection = projection
        .active_output_collection()
        .ok_or_else(|| {
            QueryError::General(format!(
                "materialized projection '{name}' has no active version"
            ))
        })?
        .to_string();
    let batches = scan::scan_limit(
        env.cassie,
        env.session,
        &output_collection,
        row_budget,
        env.controls,
    )?;
    let text_fields = projection
        .materialized
        .as_ref()
        .map(|materialized| schema_text_fields(&materialized.output_schema))
        .unwrap_or_default();
    finalize_source_batches(env, batches, text_fields, qualify, name)
}

fn execute_single_row_source(env: &SourceExecutionEnv<'_>) -> SourceExecution {
    let batches = batch::chunk_rows(vec![BatchRow::new(Vec::new())], batch::DEFAULT_BATCH_SIZE);
    ensure_query_memory_budget(env.controls, &batches)?;
    Ok((batches, Vec::new()))
}

fn execute_table_function_source(
    env: &SourceExecutionEnv<'_>,
    name: &str,
    function: &crate::sql::ast::FunctionCall,
    lateral: bool,
    outer_row: Option<&BatchRow>,
    qualify: bool,
) -> SourceExecution {
    let graph_rows =
        graph::execute_table_function(env, function, if lateral { outer_row } else { None })?;
    let (rows, _graph_memory) = graph_rows.into_parts();
    finalize_source_batches(
        env,
        batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE),
        Vec::new(),
        qualify,
        name,
    )
}

fn execute_cte_source(
    env: &SourceExecutionEnv<'_>,
    cte_context: &CteContext,
    name: &str,
    qualify: bool,
) -> SourceExecution {
    let key = name.to_ascii_lowercase();
    let rows = cte_context
        .get(&key)
        .cloned()
        .ok_or_else(|| QueryError::General(format!("relation '{name}' does not exist")))?;
    let text_fields = deduce_text_fields(&rows);
    finalize_source_batches(
        env,
        batch::chunk_rows(
            rows.into_iter().map(BatchRow::new).collect::<Vec<_>>(),
            batch::DEFAULT_BATCH_SIZE,
        ),
        text_fields,
        qualify,
        name,
    )
}

fn execute_subquery_source(
    env: &SourceExecutionEnv<'_>,
    cte_context: &CteContext,
    alias: &str,
    select: &crate::sql::ast::SelectStatement,
    lateral: bool,
    outer_row: Option<&BatchRow>,
) -> SourceExecution {
    let logical = LogicalPlan {
        command: None,
        source: select.source.clone(),
        collection: alias.to_string(),
        ctes: select.ctes.clone(),
        distinct: select.distinct,
        distinct_on: select.distinct_on.clone(),
        projection: select.projection.clone(),
        filter: select.filter.clone(),
        group_by: select.group_by.clone(),
        having: select.having.clone(),
        order: select.order.clone(),
        limit: select.limit,
        offset: select.offset,
        set: select.set.clone(),
    };
    let mut subquery_context = cte_context.clone();
    let plan_env = plan_execution_env(
        env.cassie,
        env.session,
        env.user_functions,
        env.params,
        env.controls,
    );
    let rows = execute_plan_with_outer_row(
        &plan_env,
        &logical,
        &mut subquery_context,
        if lateral { outer_row } else { None },
    )?;
    let text_fields = deduce_text_fields(
        &rows
            .iter()
            .map(|row| row.entries().to_vec())
            .collect::<Vec<_>>(),
    );
    finalize_source_batches(
        env,
        batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE),
        text_fields,
        true,
        alias,
    )
}

fn finalize_source_batches(
    env: &SourceExecutionEnv<'_>,
    mut batches: Vec<Batch>,
    text_fields: Vec<String>,
    qualify: bool,
    qualifier: &str,
) -> SourceExecution {
    if qualify {
        batches = qualify_batches(batches, qualifier);
    }
    ensure_query_memory_budget(env.controls, &batches)?;
    Ok((batches, text_fields))
}

#[path = "source_rows.rs"]
mod source_rows;
pub(crate) use source_rows::{aggregate_signature, expr_key, group_expr_name};
use source_rows::{
    apply_set_operation, combine_batches_with_outer_row, distinct_batches, distinct_on_batches,
    materialize_virtual_rows, plan_uses_aggregate, project_rows_to_schema, qualify_batches,
    schema_text_fields, source_row_budget,
};
pub(super) use source_rows::{
    combine_nulls_with_row, combine_row_with_nulls, combine_rows, qualify_row, row_columns,
    row_lookup_columns, slice_rows, source_contains_lateral,
};

pub(super) fn execute_source_query_with_outer_row(
    env: &SourceExecutionEnv<'_>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    outer_row: Option<&BatchRow>,
) -> Result<Vec<BatchRow>, QueryError> {
    check_timeout(env.controls)?;
    let started_at = Instant::now();
    if let Some(rows) = aggregate_accel::try_execute_column_batch_aggregate(
        env.cassie,
        env.session,
        plan,
        env.controls,
    )? {
        return Ok(rows);
    }
    let (mut batches, text_fields) = load_source_batches(env, plan, cte_context, outer_row)?;
    let candidate_rows = batches.iter().map(std::vec::Vec::len).sum::<usize>();
    let fulltext_fields = plan_inspection::fulltext_query_fields(plan);
    let search_context =
        build_search_context(env.cassie, plan, &batches, &text_fields, &fulltext_fields)?;
    let phase_env = PhaseExecutionEnv {
        cassie: env.cassie,
        session: env.session,
        params: env.params,
        user_functions: env.user_functions,
        controls: env.controls,
        search_context: search_context.as_ref(),
    };
    let resolved_filter = resolve_plan_filter(
        env.cassie,
        env.session,
        plan,
        cte_context,
        env.user_functions,
        env.params,
        env.controls,
    )?;
    batches = apply_filter_phase(
        batches,
        resolved_filter.as_ref(),
        env.params,
        search_context.as_ref(),
        env.user_functions,
        env.session,
        env.controls,
    )?;
    batches = apply_aggregate_phase(&phase_env, batches, plan)?;
    batches = apply_window_phase(
        batches,
        plan,
        env.params,
        search_context.as_ref(),
        env.user_functions,
        env.session,
        env.controls,
    )?;
    batches = apply_sort_phase(
        batches,
        plan,
        env.params,
        search_context.as_ref(),
        env.user_functions,
        env.session,
        env.controls,
    )?;
    batches = apply_projection_phase(
        batches,
        plan,
        env.params,
        search_context.as_ref(),
        env.user_functions,
        env.session,
        env.controls,
    )?;

    let rows = finalize_plan_rows(&phase_env, plan, cte_context, batches)?;
    record_plan_metrics(
        env.cassie,
        plan,
        &fulltext_fields,
        started_at.elapsed(),
        candidate_rows,
        rows.len(),
    );

    Ok(rows)
}

pub(super) fn source_execution_env<'a>(
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    controls: &'a QueryExecutionControls,
) -> SourceExecutionEnv<'a> {
    SourceExecutionEnv {
        cassie,
        session,
        user_functions,
        params,
        controls,
    }
}

fn load_source_batches(
    env: &SourceExecutionEnv<'_>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    outer_row: Option<&BatchRow>,
) -> SourceExecution {
    let (mut batches, text_fields) = execute_query_source(
        env,
        &plan.source,
        cte_context,
        false,
        outer_row,
        source_row_budget(plan, env.controls.max_result_rows),
    )?;
    if let Some(outer_row) = outer_row {
        batches = combine_batches_with_outer_row(batches, outer_row);
    }
    Ok((batches, text_fields))
}

fn build_search_context(
    cassie: &Cassie,
    plan: &LogicalPlan,
    batches: &[Batch],
    text_fields: &[String],
    fulltext_fields: &HashSet<String>,
) -> Result<Option<filter::SearchContext>, QueryError> {
    if fulltext_fields.is_empty() {
        return Ok(None);
    }
    let options = search_context_options(cassie, &plan.source, fulltext_fields)?;
    cassie
        .runtime
        .record_fulltext_row_scan_fallback("authoritative_row_scan");
    Ok(Some(filter::SearchContext::from_rows(
        batches.iter().flat_map(|batch| batch.iter()),
        text_fields,
        &options.boost,
        &options.k1,
        &options.b,
        &options.analyzer,
    )))
}

struct SearchContextOptions {
    boost: HashMap<String, f64>,
    k1: HashMap<String, f64>,
    b: HashMap<String, f64>,
    analyzer: HashMap<String, AnalyzerConfig>,
}

fn search_context_options(
    cassie: &Cassie,
    source: &QuerySource,
    fulltext_fields: &HashSet<String>,
) -> Result<SearchContextOptions, QueryError> {
    let QuerySource::Collection(name) = source else {
        return Ok(SearchContextOptions {
            boost: HashMap::new(),
            k1: HashMap::new(),
            b: HashMap::new(),
            analyzer: HashMap::new(),
        });
    };

    let mut field_boost = HashMap::with_capacity(cassie.catalog.text_fields(name).len());
    for field in cassie.catalog.text_fields(name) {
        if let Some(value) = cassie.catalog.get_field_boost(name, &field) {
            field_boost.insert(field, f64::from(value));
        }
    }

    let index_options = load_fulltext_index_options(cassie, name, fulltext_fields)?;
    for (field, value) in index_options.field_boost {
        field_boost.insert(field, value);
    }

    Ok(SearchContextOptions {
        boost: field_boost,
        k1: index_options.field_k1,
        b: index_options.field_b,
        analyzer: index_options.field_analyzer,
    })
}

struct PhaseExecutionEnv<'a> {
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    params: &'a [Value],
    user_functions: &'a HashMap<String, FunctionMeta>,
    controls: &'a QueryExecutionControls,
    search_context: Option<&'a filter::SearchContext>,
}

fn resolve_plan_filter(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Option<Expr>, QueryError> {
    plan.filter.as_ref().map_or(Ok(None), |filter_expr| {
        resolve_exists_expr(
            &ExistsResolutionContext {
                cassie,
                session,
                cte_context,
                user_functions,
                params,
                controls,
            },
            filter_expr,
        )
        .map(Some)
    })
}

fn apply_filter_phase(
    batches: Vec<Batch>,
    filter_expr: Option<&Expr>,
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, QueryError> {
    let Some(filter_expr) = filter_expr else {
        return Ok(batches);
    };
    let batches = filter::filter_batches(
        batches,
        filter_expr,
        params,
        search_context,
        user_functions,
        session,
    )?;
    ensure_query_memory_budget(controls, &batches)?;
    Ok(batches)
}

fn apply_aggregate_phase(
    env: &PhaseExecutionEnv<'_>,
    batches: Vec<Batch>,
    plan: &LogicalPlan,
) -> Result<Vec<Batch>, QueryError> {
    if !plan_uses_aggregate(plan) {
        return Ok(batches);
    }
    let batches = aggregate_exec::aggregate_query_batches(
        env.cassie,
        batches,
        &aggregate_exec::AggregateExecutionContext {
            plan,
            params: env.params,
            search_context: env.search_context,
            user_functions: env.user_functions,
            session: env.session,
            controls: env.controls,
        },
    )?;
    ensure_query_memory_budget(env.controls, &batches)?;
    apply_having_phase(
        batches,
        plan.having.as_ref(),
        env.params,
        env.search_context,
        env.user_functions,
        env.session,
        env.controls,
    )
}

fn apply_having_phase(
    batches: Vec<Batch>,
    having: Option<&Expr>,
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, QueryError> {
    let Some(having) = having else {
        return Ok(batches);
    };
    let having = aggregate_exec::rewrite_aggregate_expr(having);
    let batches = filter::filter_batches(
        batches,
        &having,
        params,
        search_context,
        user_functions,
        session,
    )?;
    ensure_query_memory_budget(controls, &batches)?;
    Ok(batches)
}

fn apply_window_phase(
    batches: Vec<Batch>,
    plan: &LogicalPlan,
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, QueryError> {
    let batches = window_exec::apply_window_functions(
        batches,
        &plan.projection,
        params,
        search_context,
        user_functions,
        session,
        controls,
    )?;
    ensure_query_memory_budget(controls, &batches)?;
    Ok(batches)
}

fn apply_sort_phase(
    mut batches: Vec<Batch>,
    plan: &LogicalPlan,
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, QueryError> {
    if !plan.distinct_on.is_empty() {
        let eval = sort::EvalInput {
            order: &plan.order,
            projection: &plan.projection,
            params,
            user_functions,
            session,
            search_context,
        };
        batches = sort::sort_batches_with_controls(batches, &eval, controls)?;
        ensure_query_memory_budget(controls, &batches)?;
        batches = distinct_on_batches(
            batches,
            &plan.distinct_on,
            params,
            search_context,
            user_functions,
            session,
            controls,
        )?;
        ensure_query_memory_budget(controls, &batches)?;
        return Ok(batches);
    }
    if plan.set.is_none() && !plan.order.is_empty() {
        let eval = sort::EvalInput {
            order: &plan.order,
            projection: &plan.projection,
            params,
            user_functions,
            session,
            search_context,
        };
        batches = sort::sort_batches_with_controls(batches, &eval, controls)?;
        ensure_query_memory_budget(controls, &batches)?;
    }
    Ok(batches)
}

fn apply_projection_phase(
    mut batches: Vec<Batch>,
    plan: &LogicalPlan,
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, QueryError> {
    batches = projection::project_batches(
        batches,
        &plan.projection,
        params,
        search_context,
        user_functions,
        session,
    )?;
    ensure_query_memory_budget(controls, &batches)?;
    if plan.distinct {
        batches = distinct_batches(batches, controls)?;
        ensure_query_memory_budget(controls, &batches)?;
    }
    Ok(batches)
}

fn finalize_plan_rows(
    env: &PhaseExecutionEnv<'_>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    batches: Vec<Batch>,
) -> Result<Vec<BatchRow>, QueryError> {
    let mut rows = batch::flatten_batches(batches);
    if let Some(set) = &plan.set {
        let left_output_names = set_left_output_names(env, plan, &rows);
        let right_plan = plan_inspection::logical_plan_from_select(&set.right);
        let right_rows = execute_plan(
            env.cassie,
            env.session,
            &right_plan,
            cte_context,
            env.user_functions,
            env.params,
            env.controls,
        )?;
        rows = apply_set_operation(rows, right_rows, &left_output_names, set, env.controls)?;
    }
    if plan.set.is_some() && !plan.order.is_empty() {
        let eval = sort::EvalInput {
            order: &plan.order,
            projection: &plan.projection,
            params: env.params,
            search_context: env.search_context,
            user_functions: env.user_functions,
            session: env.session,
        };
        rows = sort::sort_rows_with_controls(rows, &eval, env.controls)?;
    }
    Ok(slice_rows(rows, plan.offset, plan.limit))
}

fn set_left_output_names(
    env: &PhaseExecutionEnv<'_>,
    plan: &LogicalPlan,
    left_rows: &[BatchRow],
) -> Vec<String> {
    if let Some(row) = left_rows.first() {
        return row.entries().iter().map(|(name, _)| name.clone()).collect();
    }

    let collection_schema = env.cassie.catalog.get_schema(&plan.collection);
    aggregate::columns_from_projection(
        &plan.projection,
        collection_schema.as_ref(),
        env.user_functions,
    )
    .into_iter()
    .map(|column| column.name)
    .collect()
}

fn record_plan_metrics(
    cassie: &Cassie,
    plan: &LogicalPlan,
    fulltext_fields: &HashSet<String>,
    elapsed: std::time::Duration,
    candidate_rows: usize,
    row_count: usize,
) {
    if !fulltext_fields.is_empty() {
        cassie
            .runtime
            .record_search_execution(elapsed, candidate_rows, row_count);
    }
    if plan_inspection::plan_uses_function(plan, "hybrid_score") {
        cassie
            .runtime
            .record_hybrid_execution(elapsed, candidate_rows, row_count);
    }
    if plan_inspection::plan_uses_vector_operator(plan) {
        cassie
            .runtime
            .record_vector_execution(elapsed, candidate_rows, row_count);
    }
}
