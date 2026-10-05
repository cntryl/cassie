use crate::app::{CassieError, SessionRowCursor};
use crate::runtime::accounted::AccountedVec;

use super::{
    accounted_ordered_projection, batch, check_timeout, compare_ordered_column_candidates,
    ordered_accounting_overflow, ordered_column_heap, ordered_projection_row,
    push_ordered_column_top_k, scan, BatchRow, BinaryHeap, Cassie, CassieSession, CollectionSchema,
    DocumentRef, OrderedColumnCandidate, OrderedColumnTopKSpec, OrderedProjectionColumn,
    QueryError, QueryExecutionControls, QueryMemoryReservation, RowDecode, SelectItem, Value,
};

struct PreparedRead {
    schema: Option<CollectionSchema>,
    projection: AccountedVec<OrderedProjectionColumn>,
    cursor: SessionRowCursor,
    _schema_memory: Option<QueryMemoryReservation>,
    _scan_field_memory: QueryMemoryReservation,
    _scratch_memory: QueryMemoryReservation,
}

pub(super) fn execute(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &OrderedColumnTopKSpec<'_>,
    controls: &QueryExecutionControls,
    projection_probe: &mut impl FnMut() -> Result<(), CassieError>,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some(mut read) = prepare_read(cassie, session, spec, controls)? else {
        return Ok(None);
    };
    let (top, _heap_memory) =
        retain_candidates(cassie, spec, &mut read, controls, projection_probe)?;
    finish_candidates(cassie, spec, top, controls).map(Some)
}

fn prepare_read(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &OrderedColumnTopKSpec<'_>,
    controls: &QueryExecutionControls,
) -> Result<Option<PreparedRead>, QueryError> {
    check_timeout(controls)?;
    let (schema, schema_memory) = cassie
        .catalog
        .clone_schema_with_controls(spec.collection, controls)?;
    let catalog_name = schema.as_ref().map_or(0, |schema| {
        schema
            .fields
            .iter()
            .map(|field| field.name.len())
            .max()
            .unwrap_or(0)
    });
    let longest = spec
        .projection
        .iter()
        .filter_map(|item| match item {
            SelectItem::Column { name, .. } => Some(name.len()),
            _ => None,
        })
        .max()
        .unwrap_or(0)
        .max(spec.order_column.len());
    let scratch_bytes = longest
        .checked_mul(32)
        .and_then(|bytes| catalog_name.checked_mul(8)?.checked_add(bytes))
        .and_then(|bytes| bytes.checked_add(512))
        .ok_or_else(ordered_accounting_overflow)?;
    let scratch_memory = controls.reserve_query_memory(scratch_bytes)?;
    if schema.as_ref().is_some_and(|schema| {
        schema.fields.iter().any(|field| {
            crate::sql::ColumnIdentifierPath::stored_field_key(&field.name)
                == crate::sql::ColumnIdentifierPath::reference_field_key(spec.order_column)
                && matches!(field.data_type, crate::types::DataType::Array(_))
        })
    }) {
        // Shared typed top-k preserves elementwise ARRAY ordering.
        return Ok(None);
    }
    let projection = accounted_ordered_projection(spec.projection, controls)?;
    let (scan_fields, scan_field_memory) = spec.accounted_scan_fields(controls)?.into_parts();
    let Some(cursor) = cassie
        .open_session_row_cursor(
            session,
            spec.collection,
            RowDecode::ProjectedHistorical(scan_fields),
            controls,
        )
        .map_err(QueryError::from)?
    else {
        return Ok(None);
    };
    Ok(Some(PreparedRead {
        schema,
        projection,
        cursor,
        _schema_memory: schema_memory,
        _scan_field_memory: scan_field_memory,
        _scratch_memory: scratch_memory,
    }))
}

fn retain_candidates(
    cassie: &Cassie,
    spec: &OrderedColumnTopKSpec<'_>,
    read: &mut PreparedRead,
    controls: &QueryExecutionControls,
    projection_probe: &mut impl FnMut() -> Result<(), CassieError>,
) -> Result<(BinaryHeap<OrderedColumnCandidate>, QueryMemoryReservation), QueryError> {
    let (mut top, mut heap_memory) = ordered_column_heap(controls)?;
    loop {
        check_timeout(controls)?;
        let accounted = read
            .cursor
            .next_accounted_documents(&cassie.midge, batch::DEFAULT_BATCH_SIZE, controls)
            .map_err(QueryError::from)?;
        if accounted.is_empty() {
            break;
        }
        for document in accounted {
            check_timeout(controls)?;
            let shape = scan::ordered_projection_shape(
                document.document(),
                read.projection
                    .as_slice()
                    .iter()
                    .map(|column| (column.name.as_str(), column.output_name.as_str())),
                read.schema.as_ref(),
                Some(spec.order_column),
                controls,
            )?;
            let candidate_bytes = shape
                .bytes
                .checked_add(shape.order_value_bytes)
                .and_then(|bytes| bytes.checked_add(document.id().len()))
                .and_then(|bytes| bytes.checked_add(std::mem::size_of::<OrderedColumnCandidate>()))
                .ok_or_else(ordered_accounting_overflow)?;
            let candidate_memory = controls.reserve_query_memory(candidate_bytes)?;
            let (document, document_memory) = document.into_parts();
            let candidate =
                build_candidate(&document, spec, read, candidate_memory, projection_probe)?;
            drop(document);
            drop(document_memory);
            push_ordered_column_top_k(&mut top, spec.top_needed(), candidate, &mut heap_memory)?;
        }
    }
    Ok((top, heap_memory))
}

fn build_candidate(
    document: &DocumentRef,
    spec: &OrderedColumnTopKSpec<'_>,
    read: &PreparedRead,
    memory: QueryMemoryReservation,
    projection_probe: &mut impl FnMut() -> Result<(), CassieError>,
) -> Result<OrderedColumnCandidate, QueryError> {
    let id = document.id.clone();
    let order_value = document
        .payload
        .as_object()
        .and_then(|object| scan::projected_field_value(object, spec.order_column))
        .map_or(
            Value::Null,
            super::super::projected_read::json_to_query_value,
        );
    projection_probe()?;
    let values = ordered_projection_row(document, read.projection.as_slice(), read.schema.as_ref());
    Ok(OrderedColumnCandidate {
        order_value,
        id,
        values: values.into_entries(),
        direction: spec.direction.clone(),
        memory,
    })
}

fn finish_candidates(
    cassie: &Cassie,
    spec: &OrderedColumnTopKSpec<'_>,
    top: BinaryHeap<OrderedColumnCandidate>,
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, QueryError> {
    let mut ranked = top.into_vec();
    // IDs provide the same total tie-break without stable-sort scratch allocation.
    ranked.sort_unstable_by(compare_ordered_column_candidates);
    let output_len = ranked.len().saturating_sub(spec.offset).min(spec.limit);
    if output_len > controls.max_result_rows {
        return Err(CassieError::ResourceLimit(format!(
            "query result row limit exceeded: {output_len} > {}",
            controls.max_result_rows
        ))
        .into());
    }
    let output_bytes = output_len
        .checked_mul(
            std::mem::size_of::<BatchRow>() + std::mem::size_of::<QueryMemoryReservation>(),
        )
        .and_then(|bytes| bytes.checked_add(2 * std::mem::size_of::<Vec<BatchRow>>()))
        .ok_or_else(ordered_accounting_overflow)?;
    let _output_memory = controls.reserve_query_memory(output_bytes)?;
    let mut row_memory = output_buffer(output_len)?;
    let mut rows = output_buffer(output_len)?;
    for candidate in ranked.into_iter().skip(spec.offset).take(spec.limit) {
        check_timeout(controls)?;
        row_memory.push(candidate.memory);
        rows.push(BatchRow::new(candidate.values));
    }
    check_timeout(controls)?;
    cassie
        .runtime
        .record_read_path_heap_top_k(spec.collection, rows.len());
    Ok(rows)
}

fn output_buffer<T>(length: usize) -> Result<Vec<T>, CassieError> {
    let mut values = Vec::new();
    values.try_reserve_exact(length).map_err(|error| {
        CassieError::ResourceLimit(format!("unable to retain ordered query output: {error}"))
    })?;
    Ok(values)
}
