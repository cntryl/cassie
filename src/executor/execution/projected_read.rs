use super::*;

pub(super) fn execute_ordered_column_top_k(
    cassie: &Cassie,
    plan: &LogicalPlan,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some(spec) = ordered_column_top_k_spec(plan) else {
        return Ok(None);
    };

    let documents = cassie
        .midge
        .scan_rows_for_rebuild(
            &spec.collection,
            RowDecode::Projected(spec.projected_scan_fields()),
        )
        .map_err(|error| QueryError::General(error.to_string()))?;
    let mut top = BinaryHeap::with_capacity(spec.top_needed().saturating_add(1));

    for document in documents {
        let order_value = if is_row_id_column(&spec.order_column) {
            Value::String(document.id.clone())
        } else {
            document
                .payload
                .get(&spec.order_column)
                .map(json_to_query_value)
                .unwrap_or(Value::Null)
        };
        let values = spec
            .projection
            .iter()
            .map(|column| {
                let value = if is_row_id_column(&column.name) {
                    Value::String(document.id.clone())
                } else {
                    document
                        .payload
                        .get(&column.name)
                        .map(json_to_query_value)
                        .unwrap_or(Value::Null)
                };
                (column.output_name.clone(), value)
            })
            .collect();
        let candidate = OrderedColumnCandidate {
            order_value,
            id: document.id,
            values,
            direction: spec.direction.clone(),
        };
        push_ordered_column_top_k(&mut top, spec.top_needed(), candidate);
    }

    let mut ranked = top.into_vec();
    ranked.sort_by(compare_ordered_column_candidates);
    let rows = ranked
        .into_iter()
        .skip(spec.offset)
        .take(spec.limit)
        .map(|candidate| BatchRow::new(candidate.values))
        .collect();
    Ok(Some(rows))
}

struct OrderedColumnTopKSpec {
    collection: String,
    order_column: String,
    direction: SortDirection,
    projection: Vec<OrderedProjectionColumn>,
    limit: usize,
    offset: usize,
}

impl OrderedColumnTopKSpec {
    fn top_needed(&self) -> usize {
        self.limit.saturating_add(self.offset).max(1)
    }

    fn projected_scan_fields(&self) -> Vec<String> {
        let mut fields = Vec::new();
        if !is_row_id_column(&self.order_column) {
            fields.push(self.order_column.clone());
        }
        for column in &self.projection {
            if !is_row_id_column(&column.name) && !fields.contains(&column.name) {
                fields.push(column.name.clone());
            }
        }
        fields
    }
}

struct OrderedProjectionColumn {
    name: String,
    output_name: String,
}

fn ordered_column_top_k_spec(plan: &LogicalPlan) -> Option<OrderedColumnTopKSpec> {
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || plan.filter.is_some()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
        || plan.order.len() != 1
        || plan.order[0].nulls.is_some()
    {
        return None;
    }

    let QuerySource::Collection(collection) = &plan.source else {
        return None;
    };
    let limit = usize::try_from(plan.limit?).ok()?.max(1);
    let offset = plan
        .offset
        .and_then(|offset| usize::try_from(offset).ok())
        .unwrap_or(0);
    let Expr::Column(order_column) = &plan.order[0].expr else {
        return None;
    };
    let projection = plan
        .projection
        .iter()
        .map(|item| match item {
            SelectItem::Column { name, alias } => Some(OrderedProjectionColumn {
                name: name.clone(),
                output_name: alias.clone().unwrap_or_else(|| name.clone()),
            }),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    if projection.is_empty() {
        return None;
    }

    Some(OrderedColumnTopKSpec {
        collection: collection.clone(),
        order_column: order_column.clone(),
        direction: plan.order[0].direction.clone(),
        projection,
        limit,
        offset,
    })
}

#[derive(Debug, Clone)]
struct OrderedColumnCandidate {
    order_value: Value,
    id: String,
    values: Vec<(String, Value)>,
    direction: SortDirection,
}

impl OrderedColumnCandidate {
    fn is_better_than(&self, other: &Self) -> bool {
        compare_ordered_column_candidates(self, other) == CmpOrdering::Less
    }
}

impl PartialEq for OrderedColumnCandidate {
    fn eq(&self, other: &Self) -> bool {
        compare_ordered_column_candidates(self, other) == CmpOrdering::Equal
    }
}

impl Eq for OrderedColumnCandidate {}

impl PartialOrd for OrderedColumnCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedColumnCandidate {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        compare_ordered_column_candidates(self, other)
    }
}

fn compare_ordered_column_candidates(
    left: &OrderedColumnCandidate,
    right: &OrderedColumnCandidate,
) -> CmpOrdering {
    let value_order = compare_query_values(&left.order_value, &right.order_value);
    let value_order = match &left.direction {
        SortDirection::Asc => value_order,
        SortDirection::Desc => value_order.reverse(),
    };
    value_order.then_with(|| left.id.cmp(&right.id))
}
fn push_ordered_column_top_k(
    top: &mut BinaryHeap<OrderedColumnCandidate>,
    top_needed: usize,
    candidate: OrderedColumnCandidate,
) {
    if top.len() < top_needed {
        top.push(candidate);
    } else if let Some(worst) = top.peek() {
        if candidate.is_better_than(worst) {
            top.pop();
            top.push(candidate);
        }
    }
}

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

    let pushdown_filter = plan
        .filter
        .as_ref()
        .and_then(projected_scan_pushdown_filter);
    let mut batches = scan::scan_projected_filtered(
        cassie,
        session,
        &spec.collection,
        &spec.scan_fields,
        spec.scan_limit,
        pushdown_filter.as_ref(),
    )?;
    ensure_temp_budget(controls, &batches)?;

    if pushdown_filter.is_none() {
        if let Some(filter_expr) = &plan.filter {
            batches = filter::filter_batches(
                batches,
                filter_expr,
                params,
                None,
                user_functions,
                session,
            )?;
            ensure_temp_budget(controls, &batches)?;
        }
    }

    if !plan.order.is_empty() {
        batches = sort::sort_batches(
            batches,
            &plan.order,
            &plan.projection,
            params,
            None,
            user_functions,
            session,
        )?;
        ensure_temp_budget(controls, &batches)?;
    }

    batches = projection::project_batches(
        batches,
        &plan.projection,
        params,
        None,
        user_functions,
        session,
    )?;
    ensure_temp_budget(controls, &batches)?;

    if let Some(offset) = plan.offset {
        let offset = offset.max(0) as usize;
        let limit = plan.limit.map(|value| value.max(0) as usize);
        batches = batch::slice_batches(batches, offset, limit);
    } else if let Some(limit) = plan.limit {
        let limit = limit.max(0) as usize;
        batches = batch::slice_batches(batches, 0, Some(limit));
    }

    let rows = batch::flatten_batches(batches);
    if covering_index_for_plan(cassie, plan).is_some() {
        cassie.runtime.record_covering_index_scan(rows.len());
    } else if selected_scalar_index_for_plan(cassie, plan).is_some() {
        cassie.runtime.record_covering_index_fallback();
    }

    Ok(Some(rows))
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

    let mut breakdown = ExecutionBreakdownDurations::default();

    let scan_started = Instant::now();
    let pushdown_filter = plan
        .filter
        .as_ref()
        .and_then(projected_scan_pushdown_filter);
    let (mut batches, scan_timings) = scan::scan_projected_filtered_with_timings(
        cassie,
        session,
        &spec.collection,
        &spec.scan_fields,
        spec.scan_limit,
        pushdown_filter.as_ref(),
    )?;
    breakdown.row_decode += scan_timings.row_decode;
    let measured_scan = scan_timings.scan.saturating_add(scan_timings.row_decode);
    breakdown.scan += scan_timings
        .scan
        .saturating_add(scan_started.elapsed().saturating_sub(measured_scan));
    ensure_temp_budget(controls, &batches)?;

    if pushdown_filter.is_none() {
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
            ensure_temp_budget(controls, &batches)?;
            breakdown.filter += filter_started.elapsed();
        }
    }

    if !plan.order.is_empty() {
        let sort_started = Instant::now();
        batches = sort::sort_batches(
            batches,
            &plan.order,
            &plan.projection,
            params,
            None,
            user_functions,
            session,
        )?;
        ensure_temp_budget(controls, &batches)?;
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
    ensure_temp_budget(controls, &batches)?;
    breakdown.projection += projection_started.elapsed();

    let result_started = Instant::now();
    if let Some(offset) = plan.offset {
        let offset = offset.max(0) as usize;
        let limit = plan.limit.map(|value| value.max(0) as usize);
        batches = batch::slice_batches(batches, offset, limit);
    } else if let Some(limit) = plan.limit {
        let limit = limit.max(0) as usize;
        batches = batch::slice_batches(batches, 0, Some(limit));
    }
    let rows = batch::flatten_batches(batches);
    breakdown.result_build += result_started.elapsed();

    if covering_index_for_plan(cassie, plan).is_some() {
        cassie.runtime.record_covering_index_scan(rows.len());
    } else if selected_scalar_index_for_plan(cassie, plan).is_some() {
        cassie.runtime.record_covering_index_fallback();
    }

    Ok(Some((rows, breakdown)))
}

pub(super) struct ProjectedFilteredReadSpec {
    pub(super) collection: String,
    pub(super) scan_fields: Vec<String>,
    pub(super) scan_limit: Option<usize>,
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

fn covering_index_for_plan(cassie: &Cassie, plan: &LogicalPlan) -> Option<catalog::IndexMeta> {
    let QuerySource::Collection(collection) = &plan.source else {
        return None;
    };
    let indexes = cassie.catalog.list_indexes(collection);
    let physical = crate::planner::physical::build_with_indexes(
        plan.clone(),
        indexes.clone(),
        &Default::default(),
    );
    let selected = physical.selected_index?;
    physical
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
    let physical = crate::planner::physical::build_with_indexes(
        plan.clone(),
        indexes.clone(),
        &Default::default(),
    );
    let selected = physical.selected_index?;
    indexes.into_iter().find(|index| index.name == selected)
}

fn projected_scan_limit(limit: Option<i64>, offset: Option<i64>) -> Option<usize> {
    let limit = limit?;
    let limit = usize::try_from(limit.max(0)).ok()?;
    let offset = usize::try_from(offset.unwrap_or(0).max(0)).ok()?;
    limit.checked_add(offset)
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
        Expr::Null => Some(Value::Null),
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
        Expr::IsNull { expr, .. } => collect_projected_scan_filter_columns(expr, fields),
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
        Expr::Not { expr } => collect_projected_scan_filter_columns(expr, fields),
        Expr::Cast { expr, .. } => collect_projected_scan_filter_columns(expr, fields),
        Expr::Function(_) | Expr::Exists(_) => None,
    }
}
