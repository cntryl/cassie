use super::types::ExecutionBreakdownDurations;
use super::{
    build_select_result, dml_command, execute_physical_plan, execute_plan_with_execution_breakdown,
    materialized_projection, materialized_projection_maintenance, plan_needs_user_functions,
    rollups, Arc, Cassie, CassieSession, CteContext, ExecutionBreakdownOutput, FunctionMeta,
    HashMap, Instant, LogicalPlan, PhysicalPlan, QueryError, QueryExecutionControls, QueryResult,
    Value,
};

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn run(
    cassie: &Cassie,
    plan: PhysicalPlan,
    params: Vec<Value>,
) -> Result<QueryResult, QueryError> {
    let controls = cassie.runtime.query_controls(std::time::Instant::now());
    let plan = Arc::new(plan);
    run_with_controls(cassie, &plan, params, &controls)
}

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn run_with_controls(
    cassie: &Cassie,
    plan: &Arc<PhysicalPlan>,
    params: Vec<Value>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    run_with_session_controls(cassie, None, plan, params, controls)
}

#[doc(hidden)]
pub fn run_with_execution_breakdown(
    cassie: &Cassie,
    plan: PhysicalPlan,
    params: Vec<Value>,
) -> Result<ExecutionBreakdownOutput, QueryError> {
    let controls = cassie.runtime.query_controls(std::time::Instant::now());
    let plan = Arc::new(plan);
    run_with_execution_breakdown_controls(cassie, &plan, params, &controls)
}

fn run_with_execution_breakdown_controls(
    cassie: &Cassie,
    plan: &Arc<PhysicalPlan>,
    params: Vec<Value>,
    controls: &QueryExecutionControls,
) -> Result<ExecutionBreakdownOutput, QueryError> {
    let _entry_read_scope = crate::midge::adapter::StatementReadScope::enter(None);
    let _entry_overlay_scope = crate::app::SessionReadScope::enter(None);
    let params = params.into_boxed_slice();
    let user_functions = user_functions_for_plan(cassie, None, &plan.logical);

    if let Some(command) = plan.logical.command.as_ref() {
        let controls = controls.with_statement_read(None);
        let started = Instant::now();
        let result = dml_command::execute_command(
            cassie,
            None,
            command,
            &params,
            &user_functions,
            &controls,
        )?;
        let breakdown = ExecutionBreakdownDurations {
            result_build: started.elapsed(),
            ..Default::default()
        };
        return Ok(ExecutionBreakdownOutput {
            result,
            breakdown: breakdown.into_micros(),
        });
    }

    let statement_controls = statement_read_controls(cassie, None, controls)?;
    let controls = &statement_controls;
    let (_read_scope, _overlay_scope) = enter_statement_read(controls);
    let mut cte_context = CteContext::new();
    let (rows, mut breakdown) = execute_plan_with_execution_breakdown(
        cassie,
        None,
        &plan.logical,
        &mut cte_context,
        &user_functions,
        &params,
        controls,
    )?;

    let result_started = Instant::now();
    let result = build_select_result(cassie, plan, rows, &user_functions, controls)?;
    breakdown.result_build += result_started.elapsed();
    Ok(ExecutionBreakdownOutput {
        result,
        breakdown: breakdown.into_micros(),
    })
}

pub(crate) fn run_with_session_controls(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &Arc<PhysicalPlan>,
    params: Vec<Value>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let _entry_read_scope = crate::midge::adapter::StatementReadScope::enter(None);
    let _entry_overlay_scope = crate::app::SessionReadScope::enter(None);
    let params = params.into_boxed_slice();
    let user_functions = user_functions_for_plan(cassie, session, &plan.logical);

    if let Some(command) = plan.logical.command.as_ref() {
        let controls = controls.with_statement_read(None);
        return dml_command::execute_command(
            cassie,
            session,
            command,
            &params,
            &user_functions,
            &controls,
        );
    }

    let statement_controls = statement_read_controls(cassie, session, controls)?;
    let controls = &statement_controls;
    let (_read_scope, _overlay_scope) = enter_statement_read(controls);
    let mut cte_context = CteContext::new();
    let rows = execute_physical_plan(
        cassie,
        session,
        plan.as_ref(),
        &mut cte_context,
        &user_functions,
        &params,
        controls,
    )?;

    build_select_result(cassie, plan, rows, &user_functions, controls)
}

pub(crate) fn refresh_rollups_for_source_external(
    cassie: &Cassie,
    source: &str,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    let functions = cassie.catalog.list_functions();
    let user_functions = if cassie.database_catalog_enforced() {
        let database = crate::catalog::relation_database_name(source)
            .unwrap_or_else(|| cassie.default_database.clone());
        crate::catalog::function_resolution::functions_for_database(&functions, &database)
    } else {
        crate::catalog::function_resolution::functions_for_scope(
            &functions,
            &cassie.default_database,
            &[crate::catalog::DEFAULT_SCHEMA.to_string()],
            false,
        )
    };
    rollups::refresh_rollups_for_source(cassie, source, &user_functions, controls)
}

pub(crate) fn mark_source_projections_stale_external(
    cassie: &Cassie,
    source: &str,
) -> Result<(), QueryError> {
    materialized_projection::mark_source_projections_stale(cassie, source)
}

pub(crate) fn sync_derived_maintenance_debt_external(
    cassie: &Cassie,
    source: &str,
) -> Result<(), QueryError> {
    rollups::sync_rollup_debt_catalog(cassie, source)?;
    materialized_projection_maintenance::sync_debt_catalog(cassie, source)
}

pub(crate) fn rollup_rewrite_name_for_plan(cassie: &Cassie, plan: &LogicalPlan) -> Option<String> {
    rollups::rewrite_name_for_plan(cassie, plan)
}

fn user_functions_for_plan(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
) -> HashMap<String, FunctionMeta> {
    if plan.command.is_some() || plan_needs_user_functions(plan) {
        cassie.user_functions_for_session(session)
    } else {
        HashMap::new()
    }
}

fn statement_read_controls(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    controls: &QueryExecutionControls,
) -> Result<QueryExecutionControls, QueryError> {
    let database = session
        .and_then(|session| session.database.as_deref())
        .unwrap_or(&cassie.default_database);
    let matching = controls.statement_read().filter(|owner| {
        owner.matches(&cassie.midge, database)
            && session.is_none_or(|session| {
                owner
                    .overlay()
                    .is_some_and(|overlay| overlay.matches_session(session))
            })
    });
    let owner = if let Some(owner) = matching {
        Arc::clone(owner)
    } else if let Some(session) = session {
        session.capture_statement_read(&cassie.midge, database, controls)?
    } else {
        crate::midge::adapter::StatementDataRead::capture(&cassie.midge, database, controls)?
    };
    Ok(controls.with_statement_read(Some(owner)))
}

fn enter_statement_read(
    controls: &QueryExecutionControls,
) -> (
    crate::midge::adapter::StatementReadScope,
    crate::app::SessionReadScope,
) {
    let read_scope = crate::midge::adapter::StatementReadScope::enter(controls.statement_read());
    let overlay_scope = crate::app::SessionReadScope::enter(
        controls.statement_read().and_then(|owner| owner.overlay()),
    );
    (read_scope, overlay_scope)
}
