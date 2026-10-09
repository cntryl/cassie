use super::{
    batch, check_timeout, compare_query_values, scan, BatchRow, BinaryHeap, BinaryOp, Cassie,
    CassieSession, CmpOrdering, CollectionSchema, Expr, LogicalPlan, QueryError, SelectItem,
    SortDirection, Value,
};
use crate::midge::adapter::{DocumentRef, OrderedRowBound, RowDecode};
use crate::runtime::accounted::{Accounted, AccountedVec};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[cfg(test)]
mod tests;

#[path = "ordered_read/column_top_k.rs"]
mod column_top_k;

pub(super) fn execute_ordered_column_top_k(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    params: &[Value],
    plan: &LogicalPlan,
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    execute_ordered_column_top_k_with_projection_probe(
        cassie,
        session,
        params,
        plan,
        controls,
        || Ok(()),
    )
}

fn execute_ordered_column_top_k_with_projection_probe(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    params: &[Value],
    plan: &LogicalPlan,
    controls: &QueryExecutionControls,
    mut projection_probe: impl FnMut() -> Result<(), crate::app::CassieError>,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    if let Some(collection) = crate::sql::physical_collection(&plan.source) {
        if cassie
            .catalog
            .collection_storage_mode(collection)
            .is_some_and(
                crate::catalog::collections::CollectionStorageMode::uses_column_store_storage,
            )
        {
            return Ok(None);
        }
    }
    if let Some(rows) = execute_ordered_row_id_page(cassie, session, params, plan, controls)? {
        return Ok(Some(rows));
    }

    let Some(spec) = ordered_column_top_k_spec(plan) else {
        return Ok(None);
    };

    if spec.limit == 0 {
        return Ok(Some(Vec::new()));
    }

    column_top_k::execute(cassie, session, &spec, controls, &mut projection_probe)
}

fn execute_ordered_row_id_page(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    params: &[Value],
    plan: &LogicalPlan,
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    execute_ordered_row_id_page_with_projection_probe(
        cassie,
        session,
        params,
        plan,
        controls,
        || Ok(()),
    )
}

fn execute_ordered_row_id_page_with_projection_probe(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    params: &[Value],
    plan: &LogicalPlan,
    controls: &QueryExecutionControls,
    mut projection_probe: impl FnMut() -> Result<(), crate::app::CassieError>,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some(spec) = ordered_row_id_page_spec(plan, params) else {
        return Ok(None);
    };

    if spec.limit == 0 {
        return Ok(Some(Vec::new()));
    }

    if session.is_some_and(|session| session.has_collection_changes(spec.collection)) {
        return Ok(None);
    }

    check_timeout(controls)?;
    let (schema, _schema_memory) = cassie
        .catalog
        .clone_schema_with_controls(spec.collection, controls)?;
    let projection = spec.accounted_projection(controls)?;
    let (scan_fields, _scan_field_memory) = spec.accounted_scan_fields(controls)?.into_parts();
    let start_bound = accounted_bound(spec.start_bound.as_ref(), controls)?;
    let end_bound = accounted_bound(spec.end_bound.as_ref(), controls)?;
    let scan_limit = spec.limit.saturating_add(spec.offset);
    let mut cursor = cassie
        .midge
        .open_controlled_ordered_row_cursor(
            crate::midge::adapter::OrderedRowScanRequest {
                collection: spec.collection,
                decode: RowDecode::ProjectedHistorical(scan_fields),
                start_bound: start_bound.as_ref().map(Accounted::get),
                end_bound: end_bound.as_ref().map(Accounted::get),
                reverse: matches!(spec.direction, SortDirection::Desc),
                limit: Some(scan_limit),
            },
            controls,
        )
        .map_err(QueryError::from)?;
    let mut rows = AccountedVec::try_new(controls)?;
    let mut skipped = 0;
    while rows.len() < spec.limit {
        check_timeout(controls)?;
        let Some(document) = cursor.next_accounted_document(&cassie.midge, controls)? else {
            break;
        };
        if skipped < spec.offset {
            skipped += 1;
            continue;
        }
        if rows.len() >= controls.max_result_rows {
            return Err(crate::app::CassieError::ResourceLimit(format!(
                "query result row limit exceeded: {} > {}",
                rows.len().saturating_add(1),
                controls.max_result_rows
            ))
            .into());
        }
        let shape = scan::ordered_projection_shape(
            document.document(),
            projection
                .as_slice()
                .iter()
                .map(|column| (column.name.as_str(), column.output_name.as_str())),
            schema.as_ref(),
            None,
            controls,
        )?;
        rows.try_push_with_result(shape.bytes, || {
            projection_probe()?;
            Ok(ordered_projection_row(
                document.document(),
                projection.as_slice(),
                schema.as_ref(),
            ))
        })?;
    }
    check_timeout(controls)?;
    let (rows, _row_memory) = rows.into_parts();
    match spec.read_path_mode() {
        OrderedReadPathMode::StorageTopK => cassie
            .runtime
            .record_read_path_storage_top_k(spec.collection, rows.len()),
        OrderedReadPathMode::Keyset => cassie
            .runtime
            .record_read_path_keyset(spec.collection, rows.len()),
        OrderedReadPathMode::DegradedOffset => cassie
            .runtime
            .record_read_path_degraded_offset(spec.collection, rows.len()),
    }

    Ok(Some(rows))
}

struct OrderedColumnTopKSpec<'a> {
    collection: &'a str,
    order_column: &'a str,
    direction: SortDirection,
    projection: &'a [SelectItem],
    limit: usize,
    offset: usize,
}

impl OrderedColumnTopKSpec<'_> {
    fn top_needed(&self) -> usize {
        self.limit.saturating_add(self.offset).max(1)
    }

    fn accounted_scan_fields(
        &self,
        controls: &QueryExecutionControls,
    ) -> Result<AccountedVec<String>, QueryError> {
        let mut fields = AccountedVec::try_new(controls)?;
        if !super::projected_read::is_row_id_column(self.order_column) {
            fields.try_push_with(self.order_column.len(), || {
                crate::sql::ColumnIdentifierPath::reference_field_key(self.order_column)
            })?;
        }
        for item in self.projection {
            if let SelectItem::Column { name, .. } = item {
                if !super::projected_read::is_row_id_column(name)
                    && !fields.as_slice().contains(name)
                {
                    fields.try_push_with(name.len(), || {
                        crate::sql::ColumnIdentifierPath::reference_field_key(name)
                    })?;
                }
            }
        }
        Ok(fields)
    }
}

struct OrderedProjectionColumn {
    name: String,
    output_name: String,
}

fn ordered_projection_row(
    document: &DocumentRef,
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

struct OrderedRowIdPageSpec<'a> {
    collection: &'a str,
    direction: SortDirection,
    projection: &'a [SelectItem],
    limit: usize,
    offset: usize,
    start_bound: Option<OrderedRowBoundRef<'a>>,
    end_bound: Option<OrderedRowBoundRef<'a>>,
}

struct OrderedRowBoundRef<'a> {
    id: &'a str,
    inclusive: bool,
}

impl OrderedRowIdPageSpec<'_> {
    fn accounted_scan_fields(
        &self,
        controls: &QueryExecutionControls,
    ) -> Result<AccountedVec<String>, QueryError> {
        let mut fields = AccountedVec::try_new(controls)?;
        for item in self.projection {
            if let SelectItem::Column { name, .. } = item {
                if !super::projected_read::is_row_id_column(name) {
                    fields.try_push_with(name.len(), || {
                        crate::sql::ColumnIdentifierPath::reference_field_key(name)
                    })?;
                }
            }
        }
        Ok(fields)
    }

    fn accounted_projection(
        &self,
        controls: &QueryExecutionControls,
    ) -> Result<AccountedVec<OrderedProjectionColumn>, QueryError> {
        accounted_ordered_projection(self.projection, controls)
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

fn ordered_column_top_k_spec(plan: &LogicalPlan) -> Option<OrderedColumnTopKSpec<'_>> {
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

    let collection = crate::sql::physical_collection(&plan.source)?;
    let limit = usize::try_from(plan.limit_value()?).ok()?;
    let offset = plan
        .offset_value()
        .and_then(|offset| usize::try_from(offset).ok())
        .unwrap_or(0);
    let Expr::Column(order_column) = &plan.order[0].expr else {
        return None;
    };
    if super::projected_read::is_row_id_column(order_column) {
        return None;
    }
    if plan.projection.is_empty()
        || plan
            .projection
            .iter()
            .any(|item| !matches!(item, SelectItem::Column { .. }))
    {
        return None;
    }

    Some(OrderedColumnTopKSpec {
        collection,
        order_column,
        direction: plan.order[0].direction.clone(),
        projection: &plan.projection,
        limit,
        offset,
    })
}

fn ordered_row_id_page_spec<'a>(
    plan: &'a LogicalPlan,
    params: &'a [Value],
) -> Option<OrderedRowIdPageSpec<'a>> {
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

    let collection = crate::sql::physical_collection(&plan.source)?;
    let Expr::Column(order_column) = &plan.order[0].expr else {
        return None;
    };
    if !super::projected_read::is_row_id_column(order_column) {
        return None;
    }

    let limit = usize::try_from(plan.limit_value()?.max(0)).ok()?;
    let offset = usize::try_from(plan.offset_value().unwrap_or(0).max(0)).ok()?;
    if plan.projection.is_empty()
        || plan
            .projection
            .iter()
            .any(|item| !matches!(item, SelectItem::Column { .. }))
    {
        return None;
    }

    let (start_bound, end_bound) = match plan.filter.as_ref() {
        None => (None, None),
        Some(filter) => ordered_row_id_range_bounds(filter, params)?,
    };

    Some(OrderedRowIdPageSpec {
        collection,
        direction: plan.order[0].direction.clone(),
        projection: &plan.projection,
        limit,
        offset,
        start_bound,
        end_bound,
    })
}

fn ordered_row_id_range_bounds<'a>(
    filter: &'a Expr,
    params: &'a [Value],
) -> Option<(
    Option<OrderedRowBoundRef<'a>>,
    Option<OrderedRowBoundRef<'a>>,
)> {
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

    let row_id = match other.0 {
        Expr::StringLiteral(value) => value.as_str(),
        Expr::Param(index) => match params.get(*index)? {
            Value::String(value) => value.as_str(),
            _ => return None,
        },
        _ => return None,
    };

    match (other.1, op) {
        (false, BinaryOp::Gt) | (true, BinaryOp::Lt) => Some((
            Some(OrderedRowBoundRef {
                id: row_id,
                inclusive: false,
            }),
            None,
        )),
        (false, BinaryOp::Gte) | (true, BinaryOp::Lte) => Some((
            Some(OrderedRowBoundRef {
                id: row_id,
                inclusive: true,
            }),
            None,
        )),
        (false, BinaryOp::Lt) | (true, BinaryOp::Gt) => Some((
            None,
            Some(OrderedRowBoundRef {
                id: row_id,
                inclusive: false,
            }),
        )),
        (false, BinaryOp::Lte) | (true, BinaryOp::Gte) => Some((
            None,
            Some(OrderedRowBoundRef {
                id: row_id,
                inclusive: true,
            }),
        )),
        _ => None,
    }
}

fn accounted_bound(
    bound: Option<&OrderedRowBoundRef<'_>>,
    controls: &QueryExecutionControls,
) -> Result<Option<Accounted<OrderedRowBound>>, QueryError> {
    bound
        .map(|bound| {
            let bytes = std::mem::size_of::<OrderedRowBound>()
                .checked_add(bound.id.len())
                .ok_or_else(ordered_accounting_overflow)?;
            Accounted::try_new(controls, bytes, || OrderedRowBound {
                id: bound.id.to_owned(),
                inclusive: bound.inclusive,
            })
            .map_err(QueryError::from)
        })
        .transpose()
}

fn ordered_accounting_overflow() -> crate::app::CassieError {
    crate::app::CassieError::ResourceLimit("ordered query accounting overflow".to_owned())
}

fn accounted_ordered_projection(
    projection: &[SelectItem],
    controls: &QueryExecutionControls,
) -> Result<AccountedVec<OrderedProjectionColumn>, QueryError> {
    let mut columns = AccountedVec::try_new(controls)?;
    for item in projection {
        if let SelectItem::Column { name, alias } = item {
            let output_name = alias.as_ref().unwrap_or(name);
            let bytes = name
                .len()
                .checked_add(output_name.len())
                .ok_or_else(ordered_accounting_overflow)?;
            columns.try_push_with(bytes, || OrderedProjectionColumn {
                name: crate::sql::ColumnIdentifierPath::reference_field_key(name),
                output_name: output_name.clone(),
            })?;
        }
    }
    Ok(columns)
}

fn ordered_column_heap(
    controls: &QueryExecutionControls,
) -> Result<(BinaryHeap<OrderedColumnCandidate>, QueryMemoryReservation), QueryError> {
    let memory =
        controls.reserve_query_memory(std::mem::size_of::<BinaryHeap<OrderedColumnCandidate>>())?;
    let heap = BinaryHeap::new();
    Ok((heap, memory))
}

#[derive(Debug)]
struct OrderedColumnCandidate {
    order_value: Value,
    id: String,
    values: Vec<(String, Value)>,
    direction: SortDirection,
    memory: QueryMemoryReservation,
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
    let left_null = matches!(&left.order_value, Value::Null);
    let right_null = matches!(&right.order_value, Value::Null);
    if left_null != right_null {
        return match (&left.direction, left_null) {
            (SortDirection::Asc, true) | (SortDirection::Desc, false) => CmpOrdering::Greater,
            (SortDirection::Asc, false) | (SortDirection::Desc, true) => CmpOrdering::Less,
        };
    }

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
    heap_memory: &mut QueryMemoryReservation,
) -> Result<(), QueryError> {
    if top.len() < top_needed {
        if top.len() == top.capacity() {
            let target = if top.capacity() == 0 {
                1
            } else {
                top.capacity()
                    .checked_mul(2)
                    .unwrap_or(top_needed)
                    .min(top_needed)
            };
            let additional = target - top.capacity();
            let bytes = additional
                .checked_mul(std::mem::size_of::<OrderedColumnCandidate>())
                .ok_or_else(ordered_accounting_overflow)?;
            heap_memory.try_grow(bytes)?;
            top.try_reserve_exact(additional).map_err(|error| {
                crate::app::CassieError::ResourceLimit(format!(
                    "unable to retain ordered column heap: {error}"
                ))
            })?;
        }
        top.push(candidate);
    } else if let Some(worst) = top.peek() {
        if candidate.is_better_than(worst) {
            top.pop();
            top.push(candidate);
        }
    }
    Ok(())
}
