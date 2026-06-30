use super::{
    batch, compare_query_values, scan, BatchRow, BinaryHeap, BinaryOp, Cassie, CassieSession,
    CmpOrdering, CollectionSchema, Expr, LogicalPlan, QueryError, QuerySource, SelectItem,
    SortDirection, Value,
};
use crate::midge::adapter::{DocumentRef, OrderedRowBound, RowDecode};

pub(super) fn execute_ordered_column_top_k(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    params: &[Value],
    plan: &LogicalPlan,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    if let Some(rows) = execute_ordered_row_id_page(cassie, session, params, plan)? {
        return Ok(Some(rows));
    }

    let Some(spec) = ordered_column_top_k_spec(plan) else {
        return Ok(None);
    };

    let schema = cassie.catalog.get_schema(&spec.collection);
    let documents = if let Some(session) = session {
        cassie
            .scan_documents_batched_for_session(
                Some(session),
                &spec.collection,
                batch::DEFAULT_BATCH_SIZE,
            )
            .map_err(|error| QueryError::General(error.to_string()))?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
    } else {
        cassie
            .midge
            .scan_rows_for_rebuild(
                &spec.collection,
                RowDecode::ProjectedHistorical(spec.projected_scan_fields()),
            )
            .map_err(|error| QueryError::General(error.to_string()))?
    };
    let mut top = BinaryHeap::with_capacity(spec.top_needed().saturating_add(1));

    for document in documents {
        let document_id = document.id.clone();
        let order_value = if super::projected_read::is_row_id_column(&spec.order_column) {
            Value::String(document_id.clone())
        } else {
            document
                .payload
                .get(&spec.order_column)
                .map_or(Value::Null, super::projected_read::json_to_query_value)
        };
        let values = ordered_projection_row(document, &spec.projection, schema.as_ref());
        let candidate = OrderedColumnCandidate {
            order_value,
            id: document_id,
            values: values.into_entries(),
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
        .collect::<Vec<_>>();

    cassie
        .runtime
        .record_read_path_heap_top_k(&spec.collection, rows.len());

    Ok(Some(rows))
}

fn execute_ordered_row_id_page(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    params: &[Value],
    plan: &LogicalPlan,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some(spec) = ordered_row_id_page_spec(plan, params) else {
        return Ok(None);
    };

    if spec.limit == 0 {
        return Ok(Some(Vec::new()));
    }

    if session.is_some_and(|session| !session.collection_changes(&spec.collection).is_empty()) {
        return Ok(None);
    }

    let schema = cassie.catalog.get_schema(&spec.collection);
    let scan_limit = spec.limit.saturating_add(spec.offset);
    let (documents, _timings) = cassie
        .midge
        .scan_ordered_rows_batched_by_id_limit_with_timings(
            &spec.collection,
            batch::DEFAULT_BATCH_SIZE,
            RowDecode::ProjectedHistorical(spec.scan_fields()),
            spec.start_bound.clone(),
            spec.end_bound.clone(),
            matches!(spec.direction, SortDirection::Desc),
            Some(scan_limit),
        )
        .map_err(|error| QueryError::General(error.to_string()))?;

    let mut rows = documents
        .into_iter()
        .flatten()
        .map(|document| ordered_projection_row(document, &spec.projection, schema.as_ref()))
        .collect::<Vec<_>>();

    if spec.offset > 0 {
        rows = rows
            .into_iter()
            .skip(spec.offset)
            .take(spec.limit)
            .collect();
    }

    match spec.read_path_mode() {
        OrderedReadPathMode::StorageTopK => cassie
            .runtime
            .record_read_path_storage_top_k(&spec.collection, rows.len()),
        OrderedReadPathMode::Keyset => cassie
            .runtime
            .record_read_path_keyset(&spec.collection, rows.len()),
        OrderedReadPathMode::DegradedOffset => cassie
            .runtime
            .record_read_path_degraded_offset(&spec.collection, rows.len()),
    }

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
        if !super::projected_read::is_row_id_column(&self.order_column) {
            fields.push(self.order_column.clone());
        }
        for column in &self.projection {
            if !super::projected_read::is_row_id_column(&column.name)
                && !fields.contains(&column.name)
            {
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

fn ordered_projection_row(
    document: DocumentRef,
    projection: &[OrderedProjectionColumn],
    schema: Option<&CollectionSchema>,
) -> BatchRow {
    let projected_fields = projection
        .iter()
        .map(|column| column.name.clone())
        .collect::<Vec<_>>();
    let projected = scan::projected_document_to_row(document, &projected_fields, schema);
    let values = projection
        .iter()
        .map(|column| {
            let value = projected.get(&column.name).cloned().unwrap_or(Value::Null);
            (column.output_name.clone(), value)
        })
        .collect();
    BatchRow::from_projected_values(values)
}

enum OrderedReadPathMode {
    StorageTopK,
    Keyset,
    DegradedOffset,
}

struct OrderedRowIdPageSpec {
    collection: String,
    direction: SortDirection,
    projection: Vec<OrderedProjectionColumn>,
    limit: usize,
    offset: usize,
    start_bound: Option<OrderedRowBound>,
    end_bound: Option<OrderedRowBound>,
}

impl OrderedRowIdPageSpec {
    fn scan_fields(&self) -> Vec<String> {
        self.projection
            .iter()
            .filter(|column| !super::projected_read::is_row_id_column(&column.name))
            .map(|column| column.name.clone())
            .collect()
    }

    fn read_path_mode(&self) -> OrderedReadPathMode {
        if self.offset > 0 {
            OrderedReadPathMode::DegradedOffset
        } else if self.start_bound.is_some() || self.end_bound.is_some() {
            OrderedReadPathMode::Keyset
        } else {
            OrderedReadPathMode::StorageTopK
        }
    }
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
    if super::projected_read::is_row_id_column(order_column) {
        return None;
    }
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

fn ordered_row_id_page_spec(plan: &LogicalPlan, params: &[Value]) -> Option<OrderedRowIdPageSpec> {
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
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
    let Expr::Column(order_column) = &plan.order[0].expr else {
        return None;
    };
    if !super::projected_read::is_row_id_column(order_column) {
        return None;
    }

    let limit = usize::try_from(plan.limit?.max(0)).ok()?;
    let offset = usize::try_from(plan.offset.unwrap_or(0).max(0)).ok()?;
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

    let (start_bound, end_bound) = match plan.filter.as_ref() {
        None => (None, None),
        Some(filter) => ordered_row_id_range_bounds(filter, params)?,
    };

    Some(OrderedRowIdPageSpec {
        collection: collection.clone(),
        direction: plan.order[0].direction.clone(),
        projection,
        limit,
        offset,
        start_bound,
        end_bound,
    })
}

fn ordered_row_id_range_bounds(
    filter: &Expr,
    params: &[Value],
) -> Option<(Option<OrderedRowBound>, Option<OrderedRowBound>)> {
    let Expr::Binary { left, op, right } = filter else {
        return None;
    };

    let other = match (left.as_ref(), right.as_ref()) {
        (Expr::Column(column), other) if super::projected_read::is_row_id_column(column) => {
            (other, false)
        }
        (other, Expr::Column(column)) if super::projected_read::is_row_id_column(column) => {
            (other, true)
        }
        _ => return None,
    };

    let row_id = super::projected_read::point_lookup_value_to_row_id(other.0, params)?;

    match (other.1, op) {
        (false, BinaryOp::Gt) | (true, BinaryOp::Lt) => Some((
            Some(OrderedRowBound {
                id: row_id,
                inclusive: false,
            }),
            None,
        )),
        (false, BinaryOp::Gte) | (true, BinaryOp::Lte) => Some((
            Some(OrderedRowBound {
                id: row_id,
                inclusive: true,
            }),
            None,
        )),
        (false, BinaryOp::Lt) | (true, BinaryOp::Gt) => Some((
            None,
            Some(OrderedRowBound {
                id: row_id,
                inclusive: false,
            }),
        )),
        (false, BinaryOp::Lte) | (true, BinaryOp::Gte) => Some((
            None,
            Some(OrderedRowBound {
                id: row_id,
                inclusive: true,
            }),
        )),
        _ => None,
    }
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
