use super::types::ExecutionBreakdownDurations;
use super::{
    build_select_result, dml_command, execute_physical_plan, execute_plan_with_execution_breakdown,
    materialized_projection, plan_needs_user_functions, rollups, Arc, Cassie, CassieSession,
    CteContext, ExecutionBreakdownOutput, FunctionMeta, HashMap, Instant, LogicalPlan,
    PhysicalPlan, QueryError, QueryExecutionControls, QueryResult, Value,
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
    let user_functions = user_functions_for_plan(cassie, &plan.logical);

    if let Some(command) = plan.logical.command.as_ref() {
        let started = Instant::now();
        let result = dml_command::execute_command(
            cassie,
            None,
            command,
            &params,
            &user_functions,
            controls,
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

    let mut cte_context: CteContext = HashMap::new();
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
    let user_functions = user_functions_for_plan(cassie, &plan.logical);

    if let Some(command) = plan.logical.command.as_ref() {
        return dml_command::execute_command(
            cassie,
            session,
            command,
            &params,
            &user_functions,
            controls,
        );
    }

    let mut cte_context: CteContext = HashMap::new();
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
    let user_functions = HashMap::new();
    rollups::refresh_rollups_for_source(cassie, source, &user_functions, controls)
}

pub(crate) fn mark_source_projections_stale_external(
    cassie: &Cassie,
    source: &str,
) -> Result<(), QueryError> {
    materialized_projection::mark_source_projections_stale(cassie, source)
}

pub(crate) fn rollup_rewrite_name_for_plan(cassie: &Cassie, plan: &LogicalPlan) -> Option<String> {
    rollups::rewrite_name_for_plan(cassie, plan)
}

fn user_functions_for_plan(cassie: &Cassie, plan: &LogicalPlan) -> HashMap<String, FunctionMeta> {
    if plan.command.is_some() || plan_needs_user_functions(plan) {
        cassie
            .catalog
            .list_functions()
            .into_iter()
            .map(|metadata| (metadata.name.to_ascii_lowercase(), metadata))
            .collect::<HashMap<String, FunctionMeta>>()
    } else {
        HashMap::new()
    }
}
