#[cfg(test)]
#[path = "source_json_keys_chunk_tests.rs"]
mod source_json_keys_chunk_tests;

#[path = "jsonpath_table.rs"]
mod jsonpath_table;
#[cfg(test)]
#[path = "source_pull_tests.rs"]
mod pull_tests;
#[path = "source_graph_table.rs"]
mod source_graph_table;
pub(crate) use source_graph_table::explain_graph_sources;
#[path = "source_graph_types.rs"]
mod source_graph_types;
#[path = "source_identify_object.rs"]
mod source_identify_object;
#[path = "source_json_elements.rs"]
mod source_json_elements;
#[path = "source_json_fragment.rs"]
mod source_json_fragment;
#[path = "source_json_record_fields.rs"]
mod source_json_record_fields;
#[path = "source_json_records.rs"]
mod source_json_records;
#[path = "source_json_table.rs"]
mod source_json_table;
#[path = "source_json_table_columns.rs"]
mod source_json_table_columns;
#[path = "source_options.rs"]
mod source_options;
#[path = "source_pull.rs"]
mod source_pull;
#[path = "source_search_context.rs"]
mod source_search_context;
#[path = "source_series.rs"]
mod source_series;
#[path = "source_set_function.rs"]
pub(super) mod source_set_function;
#[path = "source_table_function.rs"]
mod source_table_function;
#[path = "source_values.rs"]
mod source_values;
use super::plan_inspection;
#[cfg(test)]
use super::reserve_projection_output_before_building;
use super::{
    aggregate, aggregate_accel, aggregate_exec, batch, catalog, check_timeout, deduce_text_fields,
    ensure_query_memory_budget, execute_plan_with_outer_row, filter, load_fulltext_index_options,
    plan_execution_env_with_parameter_oids, projection, resolve_exists_expr, row_signature, scan,
    sort, virtual_views, window_exec, AnalyzerConfig, Batch, BatchRow, Cassie, CassieSession,
    CteContext, ExistsResolutionContext, Expr, FunctionMeta, HashMap, HashSet, Instant, JoinKind,
    LogicalPlan, PlanExecutionEnv, QueryError, QueryExecutionControls, QuerySource, Value,
};
pub(super) use source_values::scratch_bytes as values_scratch_bytes;

#[path = "source_join.rs"]
mod source_join;

#[path = "source_materialized_projection.rs"]
mod materialized_source;

#[path = "source_checkpoint.rs"]
mod source_checkpoint;
#[path = "source_collection.rs"]
mod source_collection;
#[path = "source_collection_pull.rs"]
mod source_collection_pull;

#[path = "source_shape.rs"]
mod source_shape;

#[path = "source_view.rs"]
mod source_view;

#[path = "source_vector_exact.rs"]
mod source_vector_exact;

#[path = "source_table_sample.rs"]
mod source_table_sample;

#[path = "source_exists_projection.rs"]
mod exists_projection_phase;

#[path = "source_phases.rs"]
mod phases;

type SourceExecution = Result<(Vec<Batch>, Vec<String>), QueryError>;

pub(super) struct SourceExecutionEnv<'a> {
    pub(super) cassie: &'a Cassie,
    pub(super) session: Option<&'a CassieSession>,
    pub(super) binding_context: Option<&'a crate::sql::binder::BindingContext>,
    pub(super) user_functions: &'a HashMap<String, FunctionMeta>,
    pub(super) params: &'a [Value],
    pub(super) parameter_type_oids: &'a [i32],
    pub(super) controls: &'a QueryExecutionControls,
}

impl<'a> SourceExecutionEnv<'a> {
    pub(super) fn plan_env(&self) -> PlanExecutionEnv<'a> {
        if let Some(context) = self.binding_context {
            super::plan_execution_env_with_binding_context(
                self.cassie,
                self.session,
                context,
                self.user_functions,
                self.params,
                self.parameter_type_oids,
                self.controls,
            )
        } else {
            plan_execution_env_with_parameter_oids(
                self.cassie,
                self.session,
                self.user_functions,
                self.params,
                self.parameter_type_oids,
                self.controls,
            )
        }
    }

    fn phase_env<'phase>(
        &self,
        search_context: Option<&'phase filter::SearchContext>,
    ) -> PhaseExecutionEnv<'phase>
    where
        'a: 'phase,
    {
        PhaseExecutionEnv {
            cassie: self.cassie,
            session: self.session,
            binding_context: self.binding_context,
            params: self.params,
            parameter_type_oids: self.parameter_type_oids,
            user_functions: self.user_functions,
            controls: self.controls,
            search_context,
        }
    }
}

pub(super) fn execute_query_source(
    env: &SourceExecutionEnv<'_>,
    source: &QuerySource,
    cte_context: &mut CteContext,
    qualify: bool,
    outer_row: Option<&BatchRow>,
    row_budget: Option<usize>,
) -> SourceExecution {
    execute_query_source_with_traversal(
        env,
        source,
        cte_context,
        qualify,
        outer_row,
        row_budget,
        crate::midge::adapter::RowTraversal::from_limit(row_budget),
    )
}

pub(super) fn execute_query_source_with_traversal(
    env: &SourceExecutionEnv<'_>,
    source: &QuerySource,
    cte_context: &mut CteContext,
    qualify: bool,
    outer_row: Option<&BatchRow>,
    row_budget: Option<usize>,
    traversal: crate::midge::adapter::RowTraversal,
) -> SourceExecution {
    let (mut batches, text_fields) = match source {
        QuerySource::Values { rows } if rows.iter().flatten().any(Expr::contains_subquery) => {
            let rows = super::subquery_values::resolve(env, cte_context, rows, outer_row)?;
            source_values::execute(env, &rows, cte_context, outer_row, row_budget)
        }
        QuerySource::Values { rows } => {
            source_values::execute(env, rows, cte_context, outer_row, row_budget)
        }
        QuerySource::Aliased { source, alias, .. } => {
            let (batches, text_fields) = execute_query_source_with_traversal(
                env,
                source,
                cte_context,
                false,
                outer_row,
                row_budget,
                traversal,
            )?;
            let owned = batches
                .iter()
                .flatten()
                .any(|row| row.query_memory().is_some() || row.operator_memory().is_some());
            let batches = if owned {
                let (qualifier, _scratch) = source_qualification::alias_qualifier(env, alias)?;
                source_qualification::qualify_owned(env, batches, &qualifier)?
            } else {
                source_rows::qualify_batches(
                    batches,
                    &crate::sql::binder::alias_row_qualifier(alias),
                )
            };
            Ok((batches, text_fields))
        }
        QuerySource::Collection(name) => {
            execute_collection_source(env, name, qualify, row_budget, traversal)
        }
        QuerySource::TableSample {
            source,
            method,
            rate,
            repeatable,
        } => source_table_sample::execute(
            env,
            source,
            *method,
            rate,
            repeatable.as_ref(),
            cte_context,
            qualify,
        ),
        QuerySource::SingleRow => execute_single_row_source(env),
        QuerySource::TableFunction { name, .. }
            if crate::sql::table_sources::is_graph_table(name) =>
        {
            source_graph_table::execute(env, source, outer_row, qualify)
        }
        QuerySource::TableFunction { .. } => {
            source_table_function::execute(env, source, outer_row, qualify, row_budget, cte_context)
        }
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
        } => source_join::execute_join_source(
            env,
            source_join::JoinExecutionSpec {
                source,
                left,
                right,
                kind: *kind,
                on,
                outer_row,
                row_budget,
                traversal,
            },
            cte_context,
        ),
    }?;
    if !matches!(source, QuerySource::Join { .. }) {
        source_shape::attach_types(env, source, cte_context, &mut batches)?;
    }
    Ok((batches, text_fields))
}

fn execute_collection_source(
    env: &SourceExecutionEnv<'_>,
    name: &str,
    qualify: bool,
    row_budget: Option<usize>,
    traversal: crate::midge::adapter::RowTraversal,
) -> SourceExecution {
    if name
        .trim()
        .eq_ignore_ascii_case("pg_catalog.pg_projection_checkpoints")
    {
        let batches = source_checkpoint::materialize(env)?;
        return finalize_source_batches(env, batches, Vec::new(), qualify, name);
    }
    if let Some(rows) = virtual_views::rows_with_default_database(
        &env.cassie.catalog,
        &env.cassie.midge,
        name,
        env.session,
        &env.cassie.default_database,
    ) {
        return finalize_source_batches(
            env,
            materialize_virtual_rows(rows.map_err(QueryError::from)?),
            Vec::new(),
            qualify,
            name,
        );
    }

    if let Some(view) = env.cassie.catalog.get_view(name) {
        return source_view::execute_view_source(env, name, &view, qualify);
    }

    if let Some(projection) = env.cassie.catalog.get_materialized_projection(name) {
        return materialized_source::execute(env, name, &projection, qualify, row_budget);
    }

    let batches =
        source_collection::scan_collection(env, name, row_budget, qualify, name, traversal)?;
    Ok((batches, env.cassie.catalog.text_fields(name)))
}

fn execute_single_row_source(env: &SourceExecutionEnv<'_>) -> SourceExecution {
    let batches = batch::chunk_rows(vec![BatchRow::new(Vec::new())], batch::DEFAULT_BATCH_SIZE);
    ensure_query_memory_budget(env.controls, &batches)?;
    Ok((batches, Vec::new()))
}

fn execute_cte_source(
    env: &SourceExecutionEnv<'_>,
    cte_context: &CteContext,
    name: &str,
    qualify: bool,
) -> SourceExecution {
    let _key_memory = env
        .controls
        .reserve_query_memory(crate::executor::retained_memory::add(
            crate::executor::retained_memory::mul(name.len(), 2)?,
            2,
        )?)?;
    let qualifier = crate::sql::cte_identity::qualifier(name);
    let relation = cte_context
        .get(name)
        .ok_or_else(|| QueryError::General(format!("relation '{name}' does not exist")))?;
    let rows = super::cte::copy_source_rows(
        relation,
        env.controls,
        qualify.then_some(qualifier.as_str()),
    )?;
    let text_fields = deduce_text_fields(&rows);
    finalize_source_batches(
        env,
        batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE),
        text_fields,
        qualify,
        &qualifier,
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
        grouping_subtotal: select.grouping_subtotal,
        having: select.having.clone(),
        order: select.order.clone(),
        limit: select.limit.clone(),
        offset: select.offset.clone(),
        set: select.set.clone(),
    };
    // Built here rather than by the planner, so it needs the reserved-id
    // rewrite the dispatch entry points apply (see
    // `planner::logical::rewrite_reserved_id_references`); without it a bare
    // `id` in a derived table resolves to nothing, because rows carry the
    // internal identity only under `_id`.
    let mut logical = logical;
    crate::planner::logical::rewrite_reserved_id_references(&mut logical, &env.cassie.catalog);
    let controls = env.controls.for_uncapped_input();
    let mut subquery_context = cte_context.copy(&controls)?;
    let plan_env = if let Some(binding_context) = env.binding_context {
        super::plan_execution_env_with_binding_context(
            env.cassie,
            env.session,
            binding_context,
            env.user_functions,
            env.params,
            env.parameter_type_oids,
            &controls,
        )
    } else {
        plan_execution_env_with_parameter_oids(
            env.cassie,
            env.session,
            env.user_functions,
            env.params,
            env.parameter_type_oids,
            &controls,
        )
    };
    let rows = execute_plan_with_outer_row(
        &plan_env,
        &logical,
        &mut subquery_context,
        if lateral { outer_row } else { None },
    )?;
    let text_fields = deduce_text_fields(&rows);
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
        batches = source_qualification::qualify_owned(env, batches, qualifier)?;
    }
    ensure_query_memory_budget(env.controls, &batches)?;
    Ok((batches, text_fields))
}

#[path = "source_rows.rs"]
mod source_rows;
pub(crate) use source_rows::{aggregate_signature, expr_key, group_expr_name};
use source_rows::{
    apply_set_operation, attach_outer_scope, distinct_batches, distinct_on_batches,
    materialize_virtual_rows, plan_uses_aggregate, qualify_batches, schema_text_fields,
    source_row_budget,
};
pub(super) use source_rows::{
    combine_rows, qualify_row, row_lookup_columns, slice_rows, source_contains_lateral,
    source_traversal,
};

fn try_typed_aggregate(
    env: &SourceExecutionEnv<'_>,
    plan: &LogicalPlan,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    aggregate_exec::typed::try_execute(env.cassie, env.session, plan, env.params, env.controls)
}

pub(super) fn execute_source_query_with_outer_row(
    env: &SourceExecutionEnv<'_>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    outer_row: Option<&BatchRow>,
) -> Result<super::owned_rows::OwnedRows, QueryError> {
    check_timeout(env.controls)?;
    if outer_row.is_none() {
        if let Some(rows) = super::index_read::materialized_view::execute(env, plan)? {
            return Ok(super::owned_rows::OwnedRows::untracked(rows));
        }
    }
    if let Some(rows) = source_pull::try_execute(env, plan, cte_context, outer_row)? {
        return Ok(super::owned_rows::OwnedRows::untracked(rows));
    }
    let started_at = Instant::now();
    if outer_row.is_none() && plan.filter.is_some() {
        if let Some(rows) = try_typed_aggregate(env, plan)? {
            return Ok(super::owned_rows::OwnedRows::untracked(rows));
        }
    }
    if let Some(rows) = aggregate_accel::try_execute_column_batch_aggregate(
        env.cassie,
        env.session,
        plan,
        env.controls,
        env.params,
        env.user_functions,
    )? {
        return Ok(super::owned_rows::OwnedRows::untracked(rows));
    }
    if outer_row.is_none() && plan.filter.is_none() {
        if let Some(rows) = try_typed_aggregate(env, plan)? {
            return Ok(super::owned_rows::OwnedRows::untracked(rows));
        }
    }
    let loaded = source_vector_exact::load(env, plan, cte_context, outer_row)?;
    let (mut batches, text_fields) = (loaded.batches, loaded.text_fields);
    let candidate_rows = loaded
        .scanned_rows
        .unwrap_or_else(|| batches.iter().map(std::vec::Vec::len).sum::<usize>());
    let fulltext_fields = plan_inspection::SearchFields::for_plan(plan, env.params);
    let (search_context, _vocabulary_memory) =
        source_search_context::build(env, plan, &batches, &text_fields, &fulltext_fields)?;
    let phase_env = env.phase_env(search_context.as_ref());
    let resolved_filter;
    (batches, resolved_filter) =
        resolve_filter_or_correlate(env, plan, cte_context, batches, search_context.as_ref())?;
    batches = apply_filter_phase(
        batches,
        resolved_filter.as_ref(),
        env.params,
        search_context.as_ref(),
        env.user_functions,
        env.session,
        env.controls,
    )?;
    // Snapshot subtotal inputs before the legacy grouped-subquery pass can
    // merge structurally equal subqueries from independent aggregate calls.
    let subtotal = phases::prepare_subtotal(env, &phase_env, cte_context, plan, &mut batches)?;
    let plan = subtotal.as_ref().map_or(plan, |prepared| &prepared.plan);
    let grouped_subqueries = if plan_uses_aggregate(plan) {
        let search = search_context.as_ref();
        super::subquery_columns::materialize(env, cte_context, plan, &mut batches, search)?
    } else {
        None
    };
    let plan = grouped_subqueries.as_ref().unwrap_or(plan);
    batches = apply_aggregate_phase(&phase_env, batches, plan, cte_context)?;
    batches = phases::window(env, cte_context, batches, plan, search_context.as_ref())?;
    batches = phases::sort(env, cte_context, batches, plan, search_context.as_ref())?;
    let batches = if super::exists_projection::contains_routine_subquery(
        &plan.projection,
        env.user_functions,
    ) {
        projection::OwnedExpanded::untracked(exists_projection_phase::apply(
            env,
            batches,
            plan,
            cte_context,
            search_context.as_ref(),
        )?)
    } else {
        apply_projection_phase(
            batches,
            plan,
            env.params,
            search_context.as_ref(),
            env.user_functions,
            env.session,
            env.controls,
        )?
    };

    let rows = finalize_plan_rows(&phase_env, plan, cte_context, batches, outer_row)?;
    record_plan_metrics(
        env.cassie,
        plan,
        &fulltext_fields.scored,
        started_at.elapsed(),
        candidate_rows,
        rows.len(),
    );

    Ok(rows)
}

#[cfg(test)]
pub(super) fn source_execution_env<'a>(
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    controls: &'a QueryExecutionControls,
) -> SourceExecutionEnv<'a> {
    source_execution_env_with_parameter_oids(cassie, session, user_functions, params, &[], controls)
}

pub(super) fn source_execution_env_with_parameter_oids<'a>(
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    parameter_type_oids: &'a [i32],
    controls: &'a QueryExecutionControls,
) -> SourceExecutionEnv<'a> {
    SourceExecutionEnv {
        cassie,
        session,
        binding_context: None,
        user_functions,
        params,
        parameter_type_oids,
        controls,
    }
}

fn load_source_batches(
    env: &SourceExecutionEnv<'_>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    outer_row: Option<&BatchRow>,
) -> SourceExecution {
    // A lateral body keeps its own qualifiers (the binder only strips them
    // when no outer row is combined in), so its rows need qualified aliases.
    // Every plan reaches its source here, so a WHERE conjunct that guards a lateral
    // set-returning source applies before the expansion for INSERT ... SELECT, CREATE TABLE AS
    // and correlated subqueries exactly as it does for a top-level SELECT.
    let pushed = crate::planner::lateral_filter::pushed_source(plan, &|name| {
        outer_row.is_some_and(|row| row.get(name).is_some())
    });
    let (mut batches, text_fields) = execute_query_source_with_traversal(
        env,
        pushed.as_ref().unwrap_or(&plan.source),
        cte_context,
        outer_row.is_some(),
        outer_row,
        source_row_budget(plan, env.controls.max_result_rows),
        source_traversal(plan),
    )?;
    if let Some(outer_row) = outer_row {
        batches = attach_outer_scope(batches, outer_row, env.controls)?;
    }
    Ok((batches, text_fields))
}

struct PhaseExecutionEnv<'a> {
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    binding_context: Option<&'a crate::sql::binder::BindingContext>,
    params: &'a [Value],
    parameter_type_oids: &'a [i32],
    user_functions: &'a HashMap<String, FunctionMeta>,
    controls: &'a QueryExecutionControls,
    search_context: Option<&'a filter::SearchContext>,
}

/// Resolves the plan's `WHERE` EXISTS predicates once, or, when a subquery
/// references the enclosing row, filters `batches` row by row instead and
/// returns no remaining filter.
fn resolve_filter_or_correlate(
    env: &SourceExecutionEnv<'_>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    batches: Vec<Batch>,
    search_context: Option<&filter::SearchContext>,
) -> Result<(Vec<Batch>, Option<Expr>), QueryError> {
    match resolve_plan_filter(&env.plan_env(), plan, cte_context) {
        Ok(filter)
            if !filter.as_ref().is_some_and(|expr| {
                filter::expression_contains_unresolved_routine_subquery(expr, env.user_functions)
            }) =>
        {
            return Ok((batches, filter));
        }
        Ok(_) => {}
        Err(error) => {
            if !plan.filter.as_ref().is_some_and(|expr| {
                super::exists_correlated::contains_exists_or_routine_subquery(
                    expr,
                    env.user_functions,
                )
            }) {
                return Err(error);
            }
        }
    }
    let filter_expr = plan
        .filter
        .as_ref()
        .filter(|expr| {
            super::exists_correlated::contains_exists_or_routine_subquery(expr, env.user_functions)
        })
        .expect("unresolved correlated filter requires a correlated expression");
    let batches = super::exists_correlated::filter_rows_per_outer_row(
        &ExistsResolutionContext {
            cassie: env.cassie,
            session: env.session,
            binding_context: env.binding_context,
            cte_context,
            user_functions: env.user_functions,
            params: env.params,
            parameter_type_oids: env.parameter_type_oids,
            controls: env.controls,
            outer_row: None,
        },
        &plan.source,
        filter_expr,
        batches,
        search_context,
    )?;
    Ok((batches, None))
}

fn resolve_plan_filter(
    env: &PlanExecutionEnv<'_>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
) -> Result<Option<Expr>, QueryError> {
    plan.filter.as_ref().map_or(Ok(None), |filter_expr| {
        resolve_exists_expr(
            &ExistsResolutionContext {
                cassie: env.cassie,
                session: env.session,
                binding_context: env.binding_context,
                cte_context,
                user_functions: env.user_functions,
                params: env.params,
                parameter_type_oids: env.parameter_type_oids,
                controls: env.controls,
                outer_row: None,
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
    let _output_memory = ensure_query_memory_budget(controls, &batches)?;
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
    cte_context: &CteContext,
) -> Result<Vec<Batch>, QueryError> {
    if !plan_uses_aggregate(plan) {
        return Ok(batches);
    }
    let batches =
        aggregate_exec::aggregate_query_batches(env.cassie, batches, &env.aggregate_context(plan))?;
    ensure_query_memory_budget(env.controls, &batches)?;
    super::exists_phase::having(
        &SourceExecutionEnv {
            cassie: env.cassie,
            session: env.session,
            binding_context: env.binding_context,
            user_functions: env.user_functions,
            params: env.params,
            parameter_type_oids: env.parameter_type_oids,
            controls: env.controls,
        },
        cte_context,
        plan,
        batches,
        env.search_context,
    )
}

fn apply_projection_phase(
    mut batches: Vec<Batch>,
    plan: &LogicalPlan,
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    controls: &QueryExecutionControls,
) -> Result<projection::OwnedExpanded, QueryError> {
    let grouped_projection = plan_uses_aggregate(plan)
        .then(|| aggregate_exec::rewrite_aggregate_projection(&plan.projection, &plan.group_by));
    let projection = grouped_projection.as_deref().unwrap_or(&plan.projection);
    let (output_memory, _) = super::projection_handoff::ProjectionOutputMemory::admit(
        controls, &batches, projection, false, true,
    )?;
    batches = projection::project_batches(
        batches,
        projection,
        params,
        search_context,
        user_functions,
        session,
    )?;
    let mut expanded = projection::expand_json_object_keys(batches, projection, controls)?;
    let _output_memory = output_memory.retain(controls, expanded.batches_mut())?;
    ensure_query_memory_budget(controls, expanded.batches())?;
    if plan.distinct {
        expanded.transform(|batches| distinct_batches(batches, controls))?;
        ensure_query_memory_budget(controls, expanded.batches())?;
    }
    Ok(expanded)
}

fn finalize_plan_rows(
    env: &PhaseExecutionEnv<'_>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    batches: projection::OwnedExpanded,
    outer_row: Option<&BatchRow>,
) -> Result<super::owned_rows::OwnedRows, QueryError> {
    let rows = batches.flatten(env.controls)?;
    if rows.is_tracked() && plan.set.is_none() && plan.limit.is_none() && plan.offset.is_none() {
        return Ok(rows);
    }
    let mut rows = rows.into_legacy();
    if let Some(set) = &plan.set {
        let left_output_names = set_left_output_names(env, plan, &rows);
        let mut right_plan = plan_inspection::logical_plan_from_select(&set.right);
        // The left branch was rewritten with the rest of `plan`; the right
        // branch is constructed here and needs the same treatment.
        crate::planner::logical::rewrite_reserved_id_references(
            &mut right_plan,
            &env.cassie.catalog,
        );
        // The right branch is an input relation, not a result: the result-row
        // cap must not truncate it before EXCEPT/INTERSECT consult it.
        let right_controls = env.controls.for_uncapped_input();
        let right_env = if let Some(binding_context) = env.binding_context {
            super::plan_execution_env_with_binding_context(
                env.cassie,
                env.session,
                binding_context,
                env.user_functions,
                env.params,
                env.parameter_type_oids,
                &right_controls,
            )
        } else {
            plan_execution_env_with_parameter_oids(
                env.cassie,
                env.session,
                env.user_functions,
                env.params,
                env.parameter_type_oids,
                &right_controls,
            )
        };
        // A lateral derived table's set-operation branches see the same outer row as the left.
        let right_rows =
            execute_plan_with_outer_row(&right_env, &right_plan, cte_context, outer_row)?;
        rows = apply_set_operation(rows, right_rows, &left_output_names, set, env.controls)?;
    }
    if plan.set.is_some() && !plan.order.is_empty() {
        let eval = sort::EvalInput {
            order: &plan.order,
            projection: &[],
            params: env.params,
            search_context: env.search_context,
            user_functions: env.user_functions,
            session: env.session,
        };
        rows = sort::sort_set_rows_with_controls(rows, &eval, env.controls)?;
    }
    Ok(super::owned_rows::OwnedRows::untracked(slice_rows(
        rows,
        plan.offset_value(),
        plan.limit_value(),
    )))
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

#[cfg(test)]
#[path = "source_qualification_tests.rs"]
mod qualification_tests;

#[path = "source_qualification.rs"]
mod source_qualification;

#[cfg(test)]
use super::execute_plan;
#[cfg(test)]
#[path = "source_alias_accounting_tests.rs"]
mod alias_accounting_tests;

#[cfg(test)]
#[path = "source_projection_admission_tests.rs"]
mod projection_admission_tests;

#[cfg(test)]
#[path = "source_projection_scope_tests.rs"]
mod projection_scope_tests;

#[cfg(test)]
#[path = "jsonb_projection_tests.rs"]
mod jsonb_projection_tests;
