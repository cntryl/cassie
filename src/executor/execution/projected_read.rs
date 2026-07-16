use super::{
    batch, catalog, ensure_query_memory_budget, filter, projection, scan, sort, virtual_views,
    BatchRow, BinaryOp, Cassie, CassieSession, ExecutionBreakdownDurations, Expr, FunctionMeta,
    HashMap, Instant, LogicalPlan, QueryError, QueryExecutionControls, QuerySource, SelectItem,
    Value,
};

pub(super) fn is_row_id_column(column: &str) -> bool {
    column.eq_ignore_ascii_case("id") || column.eq_ignore_ascii_case("_id")
}

pub(super) fn json_to_query_value(value: &serde_json::Value) -> Value {
    if value.is_null() {
        return Value::Null;
    }
    if let Some(value) = value.as_str() {
        return Value::String(value.to_string());
    }
    if let Some(value) = value.as_bool() {
        return Value::Bool(value);
    }
    if let Some(value) = value.as_i64() {
        return Value::Int64(value);
    }
    if let Some(value) = value.as_u64().and_then(|value| i64::try_from(value).ok()) {
        return Value::Int64(value);
    }
    if let Some(value) = value.as_f64() {
        return Value::Float64(value);
    }
    Value::Json(value.clone())
}

pub(super) fn execute_projected_filtered_read(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some(spec) = projected_filtered_read_spec(plan) else {
        return Ok(None);
    };
    if virtual_views::schema(&spec.collection).is_some()
        || cassie.catalog.get_view(&spec.collection).is_some()
    {
        return Ok(None);
    }
    if let Some(rows) = super::time_series_read::try_execute_time_series_read(
        cassie,
        session,
        plan,
        user_functions,
        params,
        controls,
    )? {
        return Ok(Some(rows));
    }

    if let Some(spec) = point_lookup_read_spec(plan, params) {
        return Ok(Some(execute_projected_point_lookup_read(
            cassie,
            session,
            user_functions,
            params,
            controls,
            plan,
            &spec,
        )?));
    }

    let pushdown_filter = plan
        .filter
        .as_ref()
        .and_then(projected_scan_pushdown_filter);
    let column_filter = plan.filter.as_ref().and_then(column_batch_scan_filter);
    let scan_limit = projected_result_scan_limit(plan, controls, spec.scan_limit);
    if pushdown_filter.is_none() && column_filter.is_none() {
        if let Some(mut stream) = scan::projected_scan_stream(
            cassie,
            session,
            &spec.collection,
            &spec.scan_fields,
            scan_limit,
            controls,
        )? {
            let (rows, scan_memory) = batch::collect_batch_stream_accounted(&mut stream, controls)?;
            cassie.runtime.record_read_path_collection_scan(
                &spec.collection,
                spec.scan_fields.len(),
                rows.len(),
            );
            let mut batches = batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE);
            drop(scan_memory);
            return Ok(Some(finalize_projected_filtered_read(
                ProjectedReadFinalization {
                    cassie,
                    session,
                    plan,
                    user_functions,
                    params,
                    controls,
                    apply_filter: true,
                    apply_sort: true,
                    index_usage: None,
                },
                &mut batches,
            )?));
        }
    }
    let mut batches = scan::scan_projected_filtered(
        cassie,
        session,
        &spec.collection,
        &spec.scan_fields,
        scan_limit,
        pushdown_filter.as_ref(),
        column_filter.as_ref(),
    )?;

    let rows = batches.iter().map(Vec::len).sum::<usize>();
    cassie
        .runtime
        .record_read_path_collection_scan(&spec.collection, spec.scan_fields.len(), rows);

    Ok(Some(finalize_projected_filtered_read(
        ProjectedReadFinalization {
            cassie,
            session,
            plan,
            user_functions,
            params,
            controls,
            apply_filter: pushdown_filter.is_none(),
            apply_sort: true,
            index_usage: None,
        },
        &mut batches,
    )?))
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ProjectedReadIndexUsage {
    CoveringScalarIndex,
    SelectedScalarIndexFallback,
}

#[derive(Clone, Copy)]
pub(super) struct ProjectedReadFinalization<'a> {
    pub cassie: &'a Cassie,
    pub session: Option<&'a CassieSession>,
    pub plan: &'a LogicalPlan,
    pub user_functions: &'a HashMap<String, FunctionMeta>,
    pub params: &'a [Value],
    pub controls: &'a QueryExecutionControls,
    pub apply_filter: bool,
    pub apply_sort: bool,
    pub index_usage: Option<ProjectedReadIndexUsage>,
}

pub(super) fn finalize_projected_filtered_read(
    finalization: ProjectedReadFinalization<'_>,
    batches: &mut Vec<Vec<BatchRow>>,
) -> Result<Vec<BatchRow>, QueryError> {
    finalize_projected_filtered_read_with_index_usage(
        ProjectedReadFinalization {
            index_usage: None,
            ..finalization
        },
        batches,
    )
}

pub(super) fn finalize_projected_filtered_read_with_index_usage(
    finalization: ProjectedReadFinalization<'_>,
    batches: &mut Vec<Vec<BatchRow>>,
) -> Result<Vec<BatchRow>, QueryError> {
    let mut batch_memory = ensure_query_memory_budget(finalization.controls, batches)?;
    let mut heap_top_k_collection_name = None;
    if finalization.apply_filter {
        if let Some(filter_expr) = &finalization.plan.filter {
            let filter_started = Instant::now();
            let cloned_input_memory = ensure_query_memory_budget(finalization.controls, batches)?;
            let filtered_batches = filter::filter_batches(
                batches.clone(),
                filter_expr,
                finalization.params,
                None,
                finalization.user_functions,
                finalization.session,
            )?;
            let replacement_memory =
                ensure_query_memory_budget(finalization.controls, &filtered_batches)?;
            drop(cloned_input_memory);
            drop(batch_memory);
            *batches = filtered_batches;
            batch_memory = replacement_memory;
            let _ = filter_started;
        }
    }

    if finalization.apply_sort && !finalization.plan.order.is_empty() {
        let sort_started = Instant::now();
        let cloned_input_memory = ensure_query_memory_budget(finalization.controls, batches)?;
        let (sorted_batches, collection_name) = sort_projected_batches(
            batches.clone(),
            finalization.plan,
            finalization.params,
            finalization.user_functions,
            finalization.session,
            finalization.controls,
        )?;
        let replacement_memory =
            ensure_query_memory_budget(finalization.controls, &sorted_batches)?;
        heap_top_k_collection_name = collection_name;
        drop(cloned_input_memory);
        drop(batch_memory);
        *batches = sorted_batches;
        batch_memory = replacement_memory;
        let _ = sort_started;
    }

    let cloned_input_memory = ensure_query_memory_budget(finalization.controls, batches)?;
    let projected_batches = projection::project_batches(
        batches.clone(),
        &finalization.plan.projection,
        finalization.params,
        None,
        finalization.user_functions,
        finalization.session,
    )?;
    let replacement_memory = ensure_query_memory_budget(finalization.controls, &projected_batches)?;
    drop(cloned_input_memory);
    drop(batch_memory);
    *batches = projected_batches;
    batch_memory = replacement_memory;

    *batches = slice_batches_for_plan(
        batches.clone(),
        finalization.plan.offset,
        finalization.plan.limit,
    );

    let rows = batch::try_flatten_batches(std::mem::take(batches))?;
    drop(batch_memory);
    if let Some(collection) = heap_top_k_collection_name {
        finalization
            .cassie
            .runtime
            .record_read_path_heap_top_k(&collection, rows.len());
    }
    record_covering_index_usage(
        finalization.cassie,
        finalization.plan,
        rows.len(),
        finalization.index_usage,
    );

    Ok(rows)
}

fn execute_projected_point_lookup_read(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
    plan: &LogicalPlan,
    spec: &PointLookupReadSpec,
) -> Result<Vec<BatchRow>, QueryError> {
    let Some(document) = cassie
        .get_document_for_session(session, &spec.collection, &spec.row_id)
        .map_err(|error| QueryError::General(error.to_string()))?
    else {
        cassie
            .runtime
            .record_read_path_point_lookup(&spec.collection, false);
        return Ok(Vec::new());
    };

    cassie
        .runtime
        .record_read_path_point_lookup(&spec.collection, true);
    let schema = cassie.catalog.get_schema(&spec.collection);
    let row = scan::projected_document_to_row(document, &spec.scan_fields, schema.as_ref());
    let mut batches = vec![vec![row]];

    finalize_projected_filtered_read(
        ProjectedReadFinalization {
            cassie,
            session,
            plan,
            user_functions,
            params,
            controls,
            apply_filter: false,
            apply_sort: true,
            index_usage: None,
        },
        &mut batches,
    )
}

pub(super) fn execute_projected_filtered_read_with_breakdown(
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
    if let Some(rows) = super::time_series_read::try_execute_time_series_read(
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
            batches = filter::filter_batches(
                batches,
                filter_expr,
                params,
                None,
                user_functions,
                session,
            )?;
            ensure_query_memory_budget(controls, &batches)?;
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
        ensure_query_memory_budget(controls, &batches)?;
        breakdown.sort += sort_started.elapsed();
    }

    let projection_started = Instant::now();
    batches = projection::project_batches(
        batches,
        &plan.projection,
        params,
        None,
        user_functions,
        session,
    )?;
    ensure_query_memory_budget(controls, &batches)?;
    breakdown.projection += projection_started.elapsed();

    let result_started = Instant::now();
    batches = slice_batches_for_plan(batches, plan.offset, plan.limit);
    let rows = batch::try_flatten_batches(batches)?;
    breakdown.result_build += result_started.elapsed();

    if let Some(collection) = heap_top_k_collection_name {
        cassie
            .runtime
            .record_read_path_heap_top_k(&collection, rows.len());
    }
    record_covering_index_usage(cassie, plan, rows.len(), None);

    Ok(Some((rows, breakdown)))
}

pub(super) struct ProjectedFilteredReadSpec {
    pub(super) collection: String,
    pub(super) scan_fields: Vec<String>,
    pub(super) scan_limit: Option<usize>,
}

#[derive(Debug, Clone)]
struct PointLookupReadSpec {
    collection: String,
    row_id: String,
    scan_fields: Vec<String>,
}

pub(super) fn projected_filtered_read_spec(
    plan: &LogicalPlan,
) -> Option<ProjectedFilteredReadSpec> {
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
    {
        return None;
    }

    let QuerySource::Collection(collection) = &plan.source else {
        return None;
    };
    let filter_columns = match plan.filter.as_ref() {
        Some(filter) => projected_scan_filter_columns(filter)?,
        None => Vec::new(),
    };
    let projection_columns = plan
        .projection
        .iter()
        .map(|item| match item {
            SelectItem::Column { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    if projection_columns.is_empty() {
        return None;
    }
    let order_columns = projected_order_columns(plan)?;

    let mut scan_fields = Vec::new();
    for column in projection_columns
        .into_iter()
        .chain(filter_columns)
        .chain(order_columns)
    {
        if is_row_id_column(&column) || scan_fields.contains(&column) {
            continue;
        }
        scan_fields.push(column);
    }

    let scan_limit = if plan.filter.is_none() {
        projected_scan_limit(plan.limit, plan.offset)
    } else {
        None
    };

    Some(ProjectedFilteredReadSpec {
        collection: collection.clone(),
        scan_fields,
        scan_limit,
    })
}

fn point_lookup_read_spec(plan: &LogicalPlan, params: &[Value]) -> Option<PointLookupReadSpec> {
    if plan.offset.is_some_and(|offset| offset > 0) {
        return None;
    }

    let projected = projected_filtered_read_spec(plan)?;
    let filter = plan.filter.as_ref()?;
    let row_id = point_lookup_row_id(filter, params)?;

    Some(PointLookupReadSpec {
        collection: projected.collection,
        row_id,
        scan_fields: projected.scan_fields,
    })
}

fn point_lookup_row_id(filter: &Expr, params: &[Value]) -> Option<String> {
    let Expr::Binary {
        left,
        op: BinaryOp::Eq,
        right,
    } = filter
    else {
        return None;
    };

    match (left.as_ref(), right.as_ref()) {
        (Expr::Column(column), value) if is_row_id_column(column) => {
            point_lookup_value_to_row_id(value, params)
        }
        (value, Expr::Column(column)) if is_row_id_column(column) => {
            point_lookup_value_to_row_id(value, params)
        }
        _ => None,
    }
}

pub(super) fn point_lookup_value_to_row_id(expr: &Expr, params: &[Value]) -> Option<String> {
    match expr {
        Expr::StringLiteral(value) => Some(value.clone()),
        Expr::NumberLiteral(value) => row_id_from_number(*value),
        Expr::BoolLiteral(value) => Some(value.to_string()),
        Expr::Param(index) => params.get(*index).and_then(row_id_from_value),
        _ => None,
    }
}

fn row_id_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Int64(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Float64(value) => Some(value.to_string()),
        _ => None,
    }
}

fn row_id_from_number(value: f64) -> Option<String> {
    if !value.is_finite() {
        return None;
    }

    if value.fract() == 0.0 {
        if let Ok(integer) = format!("{value:.0}").parse::<i64>() {
            return Some(integer.to_string());
        }
    }

    Some(value.to_string())
}

fn projected_order_columns(plan: &LogicalPlan) -> Option<Vec<String>> {
    let mut fields = Vec::new();
    for order in &plan.order {
        let Expr::Column(column) = &order.expr else {
            return None;
        };
        if !fields.iter().any(|field: &String| field == column) {
            fields.push(column.clone());
        }
    }
    Some(fields)
}

fn sort_projected_batches(
    batches: Vec<Vec<BatchRow>>,
    plan: &LogicalPlan,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    controls: &QueryExecutionControls,
) -> Result<(Vec<Vec<BatchRow>>, Option<String>), QueryError> {
    if let Some(top_needed) = projected_scan_limit(plan.limit, plan.offset) {
        let eval = sort::EvalInput {
            order: &plan.order,
            projection: &plan.projection,
            params,
            search_context: None,
            user_functions,
            session,
        };
        return Ok((
            sort::top_k_batches_with_controls(batches, &eval, top_needed, controls)?,
            heap_top_k_collection(plan),
        ));
    }

    let eval = sort::EvalInput {
        order: &plan.order,
        projection: &plan.projection,
        params,
        search_context: None,
        user_functions,
        session,
    };
    Ok((
        sort::sort_batches_with_controls(batches, &eval, controls)?,
        None,
    ))
}

fn covering_index_for_plan(cassie: &Cassie, plan: &LogicalPlan) -> Option<catalog::IndexMeta> {
    let QuerySource::Collection(collection) = &plan.source else {
        return None;
    };
    let indexes = cassie.catalog.list_indexes(collection);
    let cardinality_stats =
        std::collections::HashMap::<String, crate::catalog::CollectionCardinalityStats>::new();
    let physical = crate::planner::physical::build_with_indexes(
        plan.clone(),
        indexes.as_slice(),
        &cardinality_stats,
    );
    let selected = physical.read.selected_index?;
    physical
        .read
        .covered_index
        .then(|| indexes.into_iter().find(|index| index.name == selected))
        .flatten()
}

fn selected_scalar_index_for_plan(
    cassie: &Cassie,
    plan: &LogicalPlan,
) -> Option<catalog::IndexMeta> {
    let QuerySource::Collection(collection) = &plan.source else {
        return None;
    };
    let indexes = cassie.catalog.list_indexes(collection);
    let cardinality_stats =
        std::collections::HashMap::<String, crate::catalog::CollectionCardinalityStats>::new();
    let physical = crate::planner::physical::build_with_indexes(
        plan.clone(),
        indexes.as_slice(),
        &cardinality_stats,
    );
    let selected = physical.read.selected_index?;
    indexes.into_iter().find(|index| index.name == selected)
}

fn projected_scan_limit(limit: Option<i64>, offset: Option<i64>) -> Option<usize> {
    let limit = limit?;
    let limit = usize::try_from(limit.max(0)).ok()?;
    let offset = usize::try_from(offset.unwrap_or(0).max(0)).ok()?;
    limit.checked_add(offset)
}

fn projected_result_scan_limit(
    plan: &LogicalPlan,
    controls: &QueryExecutionControls,
    planned_limit: Option<usize>,
) -> Option<usize> {
    if plan.filter.is_some() || !plan.order.is_empty() {
        return planned_limit;
    }
    let result_cap = controls.max_result_rows.saturating_add(1);
    Some(planned_limit.unwrap_or(result_cap).min(result_cap))
}

fn heap_top_k_collection(plan: &LogicalPlan) -> Option<String> {
    match &plan.source {
        QuerySource::Collection(collection) => Some(collection.clone()),
        _ => None,
    }
}

pub(super) fn projected_scan_pushdown_filter(expr: &Expr) -> Option<scan::ProjectedDocumentFilter> {
    let Expr::Binary {
        left,
        op: BinaryOp::Eq,
        right,
    } = expr
    else {
        return None;
    };

    match (left.as_ref(), right.as_ref()) {
        (Expr::Column(field), literal) => {
            if is_row_id_column(field) {
                return None;
            }
            projected_pushdown_literal(literal).map(|value| scan::ProjectedDocumentFilter {
                field: field.clone(),
                value,
            })
        }
        (literal, Expr::Column(field)) => {
            if is_row_id_column(field) {
                return None;
            }
            projected_pushdown_literal(literal).map(|value| scan::ProjectedDocumentFilter {
                field: field.clone(),
                value,
            })
        }
        _ => None,
    }
}

fn projected_pushdown_literal(expr: &Expr) -> Option<Value> {
    match expr {
        Expr::StringLiteral(value) => Some(Value::String(value.clone())),
        Expr::BoolLiteral(value) => Some(Value::Bool(*value)),
        _ => None,
    }
}

fn column_batch_scan_filter(expr: &Expr) -> Option<crate::midge::adapter::ColumnBatchScanFilter> {
    let mut predicates = Vec::new();
    collect_column_batch_predicates(expr, &mut predicates)?;
    (!predicates.is_empty()).then_some(crate::midge::adapter::ColumnBatchScanFilter { predicates })
}

fn collect_column_batch_predicates(
    expr: &Expr,
    predicates: &mut Vec<crate::midge::adapter::ColumnBatchScanPredicate>,
) -> Option<()> {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => {
            collect_column_batch_predicates(left, predicates)?;
            collect_column_batch_predicates(right, predicates)
        }
        Expr::Binary { left, op, right } => {
            let (field, op, value) = column_batch_binary_predicate(left, op, right)?;
            predicates.push(crate::midge::adapter::ColumnBatchScanPredicate {
                field,
                op,
                value: Some(value),
            });
            Some(())
        }
        Expr::Between {
            expr,
            low,
            high,
            negated: false,
        } => {
            let Expr::Column(field) = expr.as_ref() else {
                return None;
            };
            if is_row_id_column(field) {
                return None;
            }
            if matches!(low.as_ref(), Expr::Null) || matches!(high.as_ref(), Expr::Null) {
                return None;
            }
            predicates.push(crate::midge::adapter::ColumnBatchScanPredicate {
                field: field.clone(),
                op: crate::midge::adapter::ColumnBatchScanOp::Gte,
                value: Some(column_batch_literal(low)?),
            });
            predicates.push(crate::midge::adapter::ColumnBatchScanPredicate {
                field: field.clone(),
                op: crate::midge::adapter::ColumnBatchScanOp::Lte,
                value: Some(column_batch_literal(high)?),
            });
            Some(())
        }
        Expr::IsNull { expr, negated } => {
            let Expr::Column(field) = expr.as_ref() else {
                return None;
            };
            if is_row_id_column(field) {
                return None;
            }
            predicates.push(crate::midge::adapter::ColumnBatchScanPredicate {
                field: field.clone(),
                op: if *negated {
                    crate::midge::adapter::ColumnBatchScanOp::IsNotNull
                } else {
                    crate::midge::adapter::ColumnBatchScanOp::IsNull
                },
                value: None,
            });
            Some(())
        }
        _ => None,
    }
}

fn column_batch_binary_predicate(
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
) -> Option<(
    String,
    crate::midge::adapter::ColumnBatchScanOp,
    serde_json::Value,
)> {
    match (left, right) {
        (Expr::Column(field), literal) if !is_row_id_column(field) => {
            let value = column_batch_literal(literal)?;
            let scan_op = column_batch_scan_op(op)?;
            (!value.is_null()).then_some((field.clone(), scan_op, value))
        }
        (literal, Expr::Column(field)) if !is_row_id_column(field) => {
            let value = column_batch_literal(literal)?;
            let scan_op = reverse_column_batch_scan_op(op)?;
            (!value.is_null()).then_some((field.clone(), scan_op, value))
        }
        _ => None,
    }
}

fn column_batch_scan_op(op: &BinaryOp) -> Option<crate::midge::adapter::ColumnBatchScanOp> {
    match op {
        BinaryOp::Eq => Some(crate::midge::adapter::ColumnBatchScanOp::Eq),
        BinaryOp::Lt => Some(crate::midge::adapter::ColumnBatchScanOp::Lt),
        BinaryOp::Lte => Some(crate::midge::adapter::ColumnBatchScanOp::Lte),
        BinaryOp::Gt => Some(crate::midge::adapter::ColumnBatchScanOp::Gt),
        BinaryOp::Gte => Some(crate::midge::adapter::ColumnBatchScanOp::Gte),
        _ => None,
    }
}

fn reverse_column_batch_scan_op(op: &BinaryOp) -> Option<crate::midge::adapter::ColumnBatchScanOp> {
    match op {
        BinaryOp::Eq => Some(crate::midge::adapter::ColumnBatchScanOp::Eq),
        BinaryOp::Lt => Some(crate::midge::adapter::ColumnBatchScanOp::Gt),
        BinaryOp::Lte => Some(crate::midge::adapter::ColumnBatchScanOp::Gte),
        BinaryOp::Gt => Some(crate::midge::adapter::ColumnBatchScanOp::Lt),
        BinaryOp::Gte => Some(crate::midge::adapter::ColumnBatchScanOp::Lte),
        _ => None,
    }
}

fn column_batch_literal(expr: &Expr) -> Option<serde_json::Value> {
    match expr {
        Expr::StringLiteral(value) => Some(serde_json::Value::String(value.clone())),
        Expr::BoolLiteral(value) => Some(serde_json::Value::Bool(*value)),
        Expr::NumberLiteral(value) => {
            serde_json::Number::from_f64(*value).map(serde_json::Value::Number)
        }
        Expr::Null => Some(serde_json::Value::Null),
        _ => None,
    }
}

fn projected_scan_filter_columns(expr: &Expr) -> Option<Vec<String>> {
    let mut fields = Vec::new();
    collect_projected_scan_filter_columns(expr, &mut fields)?;
    Some(fields)
}

fn collect_projected_scan_filter_columns(expr: &Expr, fields: &mut Vec<String>) -> Option<()> {
    match expr {
        Expr::Column(name) => {
            if !fields.iter().any(|field| field.eq_ignore_ascii_case(name)) {
                fields.push(name.clone());
            }
            Some(())
        }
        Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null => Some(()),
        Expr::Binary {
            left, op, right, ..
        } => {
            match op {
                crate::sql::ast::BinaryOp::Eq
                | crate::sql::ast::BinaryOp::NotEq
                | crate::sql::ast::BinaryOp::Lt
                | crate::sql::ast::BinaryOp::Lte
                | crate::sql::ast::BinaryOp::Gt
                | crate::sql::ast::BinaryOp::Gte
                | crate::sql::ast::BinaryOp::And
                | crate::sql::ast::BinaryOp::Or
                | crate::sql::ast::BinaryOp::Like => {}
                _ => return None,
            }
            collect_projected_scan_filter_columns(left, fields)?;
            collect_projected_scan_filter_columns(right, fields)
        }
        Expr::IsNull { expr, .. } | Expr::Not { expr } | Expr::Cast { expr, .. } => {
            collect_projected_scan_filter_columns(expr, fields)
        }
        Expr::InList { expr, values, .. } => {
            collect_projected_scan_filter_columns(expr, fields)?;
            for value in values {
                collect_projected_scan_filter_columns(value, fields)?;
            }
            Some(())
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            collect_projected_scan_filter_columns(expr, fields)?;
            collect_projected_scan_filter_columns(low, fields)?;
            collect_projected_scan_filter_columns(high, fields)
        }
        Expr::Function(_) | Expr::Exists(_) => None,
    }
}

struct ProjectedReadScan {
    batches: Vec<Vec<BatchRow>>,
    scan_timings: scan::ScanTimings,
    started: Instant,
    pushdown_filter_absent: bool,
}

fn scan_projected_read_batches(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &ProjectedFilteredReadSpec,
    plan: &LogicalPlan,
    controls: &QueryExecutionControls,
) -> Result<ProjectedReadScan, QueryError> {
    let started = Instant::now();
    let pushdown_filter = plan
        .filter
        .as_ref()
        .and_then(projected_scan_pushdown_filter);
    let column_filter = plan.filter.as_ref().and_then(column_batch_scan_filter);
    let scan_limit = projected_result_scan_limit(plan, controls, spec.scan_limit);
    let (batches, scan_timings) = scan::scan_projected_filtered_with_timings(
        cassie,
        session,
        &spec.collection,
        &spec.scan_fields,
        scan_limit,
        pushdown_filter.as_ref(),
        column_filter.as_ref(),
    )?;
    let rows = batches.iter().map(Vec::len).sum::<usize>();
    cassie
        .runtime
        .record_read_path_collection_scan(&spec.collection, spec.scan_fields.len(), rows);
    ensure_query_memory_budget(controls, &batches)?;
    Ok(ProjectedReadScan {
        batches,
        scan_timings,
        started,
        pushdown_filter_absent: pushdown_filter.is_none(),
    })
}

fn slice_batches_for_plan(
    batches: Vec<Vec<BatchRow>>,
    offset: Option<i64>,
    limit: Option<i64>,
) -> Vec<Vec<BatchRow>> {
    let offset = offset.and_then(plan_bound_to_usize).unwrap_or(0);
    let limit = limit.and_then(plan_bound_to_usize);
    batch::slice_batches(batches, offset, limit)
}

fn plan_bound_to_usize(value: i64) -> Option<usize> {
    usize::try_from(value.max(0)).ok()
}

fn record_covering_index_usage(
    cassie: &Cassie,
    plan: &LogicalPlan,
    row_count: usize,
    index_usage: Option<ProjectedReadIndexUsage>,
) {
    match index_usage {
        Some(ProjectedReadIndexUsage::CoveringScalarIndex) => {
            cassie.runtime.record_covering_index_scan(row_count);
        }
        Some(ProjectedReadIndexUsage::SelectedScalarIndexFallback) => {
            cassie.runtime.record_covering_index_fallback();
        }
        None => {
            if covering_index_for_plan(cassie, plan).is_some() {
                cassie.runtime.record_covering_index_scan(row_count);
            } else if selected_scalar_index_for_plan(cassie, plan).is_some() {
                cassie.runtime.record_covering_index_fallback();
            }
        }
    }
}
