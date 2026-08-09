use super::{
    batch, ensure_query_memory_budget, execute_projected_point_lookup_read, filter,
    point_lookup_read_spec, projected_filtered_read_spec, projection, record_covering_index_usage,
    reserve_projection_output_before_building, scan_projected_read_batches, slice_batches_for_plan,
    sort_projected_batches, virtual_views, BatchRow, Cassie, CassieSession,
    ExecutionBreakdownDurations, FunctionMeta, HashMap, Instant, LogicalPlan, QueryError,
    QueryExecutionControls, Value,
};
use crate::executor::execution::time_series_read;

pub(in crate::executor::execution) fn execute_projected_filtered_read_with_breakdown(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Option<(Vec<BatchRow>, ExecutionBreakdownDurations)>, QueryError> {
    let Some(spec) = projected_filtered_read_spec(plan) else {
        return Ok(None);
    };
    if virtual_views::schema(&spec.collection).is_some()
        || cassie.catalog.get_view(&spec.collection).is_some()
    {
        return Ok(None);
    }
    if let Some(rows) = time_series_read::try_execute_time_series_read(
        cassie,
        session,
        plan,
        user_functions,
        params,
        controls,
    )? {
        return Ok(Some((rows, ExecutionBreakdownDurations::default())));
    }

    let mut breakdown = ExecutionBreakdownDurations::default();

    if let Some(spec) = point_lookup_read_spec(plan, params) {
        let result_started = Instant::now();
        let rows = execute_projected_point_lookup_read(
            cassie,
            session,
            user_functions,
            params,
            controls,
            plan,
            &spec,
        )?;
        breakdown.result_build += result_started.elapsed();
        return Ok(Some((rows, breakdown)));
    }

    let scan = scan_projected_read_batches(cassie, session, &spec, plan, controls)?;
    let mut batches = scan.batches;
    let mut batch_memory = ensure_query_memory_budget(controls, &batches)?;
    breakdown.row_decode += scan.scan_timings.row_decode;
    let measured_scan = scan
        .scan_timings
        .scan
        .saturating_add(scan.scan_timings.row_decode);
    breakdown.scan += scan
        .scan_timings
        .scan
        .saturating_add(scan.started.elapsed().saturating_sub(measured_scan));

    if scan.pushdown_filter_absent {
        if let Some(filter_expr) = &plan.filter {
            let filter_started = Instant::now();
            let cloned_input_memory = ensure_query_memory_budget(controls, &batches)?;
            let filtered_batches = filter::filter_batches(
                batches.clone(),
                filter_expr,
                params,
                None,
                user_functions,
                session,
            )?;
            drop(batch_memory);
            batches = filtered_batches;
            batch_memory = cloned_input_memory;
            breakdown.filter += filter_started.elapsed();
        }
    }

    let mut heap_top_k_collection_name = None;
    if !plan.order.is_empty() {
        let sort_started = Instant::now();
        let (sorted_batches, collection_name) =
            sort_projected_batches(batches, plan, params, user_functions, session, controls)?;
        batches = sorted_batches;
        heap_top_k_collection_name = collection_name;
        drop(batch_memory);
        batch_memory = ensure_query_memory_budget(controls, &batches)?;
        breakdown.sort += sort_started.elapsed();
    }

    let projection_started = Instant::now();
    let cloned_input_memory = ensure_query_memory_budget(controls, &batches)?;
    let projected_output_memory =
        reserve_projection_output_before_building(controls, &batches, &plan.projection)?;
    let projected_batches = projection::project_batches(
        batches.clone(),
        &plan.projection,
        params,
        None,
        user_functions,
        session,
    )?;
    drop(cloned_input_memory);
    drop(batch_memory);
    batches = projected_batches;
    batch_memory = projected_output_memory;
    breakdown.projection += projection_started.elapsed();

    let result_started = Instant::now();
    batches = slice_batches_for_plan(batches, plan.offset, plan.limit);
    let rows = batch::try_flatten_batches(batches)?;
    drop(batch_memory);
    breakdown.result_build += result_started.elapsed();

    record_breakdown_read_path(cassie, plan, heap_top_k_collection_name, rows.len());

    Ok(Some((rows, breakdown)))
}

fn record_breakdown_read_path(
    cassie: &Cassie,
    plan: &LogicalPlan,
    heap_top_k_collection_name: Option<String>,
    row_count: usize,
) {
    if let Some(collection) = heap_top_k_collection_name {
        cassie
            .runtime
            .record_read_path_heap_top_k(&collection, row_count);
    }
    record_covering_index_usage(cassie, plan, row_count, None);
}
