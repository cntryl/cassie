//! Admit EXISTS RETURNING copies through the complete public materialization boundary.
use std::mem::size_of;
use std::sync::Arc;

use super::super::projection_handoff::ProjectionOutputMemory;
use super::super::{
    check_timeout, exists_projection, scan, source, Batch, CteContext, QuerySource,
};
use super::{
    dml_returning_columns, BatchRow, DataType, DmlResultContext, QueryError, QueryResult, Value,
};
use crate::executor::batch::OperatorMemory;
use crate::executor::retained_memory::{add, mul};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

pub(super) fn build(
    context: &DmlResultContext<'_>,
    affected_count: usize,
    mut rows: Vec<BatchRow>,
) -> Result<QueryResult, QueryError> {
    let (schema, _schema_memory) = context
        .cassie
        .catalog
        .clone_schema_with_controls(context.table, context.controls)?;
    if schema.as_ref().is_some_and(|schema| {
        schema
            .fields
            .iter()
            .any(|field| matches!(field.data_type, DataType::Array(_)))
    }) {
        if let Some(first) = rows.first_mut() {
            scan::attach_row_types(first, schema.as_ref());
            if let Some(types) = first.shared_data_types() {
                for row in &mut rows[1..] {
                    row.set_data_types(types.clone());
                }
            }
        }
    }
    let capacity = rows.capacity();
    admit_inputs(&mut rows, capacity, context.controls)?;
    let mut batches = vec![rows];
    let (memory, _scratch) =
        ProjectionOutputMemory::admit(context.controls, &batches, context.returning, true, true)?;
    let source = QuerySource::Collection(
        crate::sql::IdentifierPath::parse(context.table).map_err(QueryError::General)?,
    );
    let env = source::SourceExecutionEnv {
        cassie: context.cassie,
        session: context.session,
        user_functions: context.user_functions,
        params: context.params,
        controls: context.statement_read_controls,
    };
    {
        let (_read_scope, _overlay_scope) =
            super::super::entrypoints::enter_statement_read(context.statement_read_controls);
        batches = exists_projection::project(
            &env,
            &CteContext::new(),
            &source,
            batches,
            context.returning,
            None,
        )?;
    }
    let _memory = memory.retain(context.controls, &mut batches)?;
    let count = batches
        .iter()
        .try_fold(0, |count, batch| add(count, batch.len()))?;
    let mut projected = Vec::new();
    projected
        .try_reserve_exact(count)
        .map_err(allocation_error)?;
    projected.extend(batches.into_iter().flatten());
    #[cfg(test)]
    super::dml_returning_tests::observe(
        super::dml_returning_tests::Phase::Projected,
        context.controls,
        affected_count,
        &projected,
        projected.capacity(),
    );
    let columns = dml_returning_columns(context.returning, schema.as_ref(), context.user_functions);
    let rows = materialize(projected, context.controls, affected_count)?;
    Ok(QueryResult {
        columns,
        rows,
        command: format!("{} {affected_count}", context.command_prefix),
    })
}

fn admit_inputs(
    rows: &mut [BatchRow],
    capacity: usize,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    check_timeout(controls)?;
    let slots = add(mul(capacity, size_of::<BatchRow>())?, size_of::<Batch>())?;
    let bytes = rows.iter().try_fold(
        add(
            slots,
            size_of::<QueryMemoryReservation>() + 2 * size_of::<usize>(),
        )?,
        |bytes, row| {
            add(
                bytes,
                add(row.unleased_body_bytes()?, row.pending_lookup_bytes()?)?,
            )
        },
    )?;
    let memory = Arc::new(controls.reserve_query_memory(bytes)?);
    for row in rows {
        check_timeout(controls)?;
        row.attach_operator_memory(controls, Arc::clone(&memory))?;
    }
    Ok(())
}

fn materialize(
    rows: Vec<BatchRow>,
    controls: &QueryExecutionControls,
    affected_count: usize,
) -> Result<Vec<Vec<Value>>, QueryError> {
    let _ = affected_count;
    let _parents = OperatorMemory::hold_rows(controls, &rows)?;
    let bytes = rows
        .iter()
        .try_fold(mul(rows.len(), size_of::<Vec<Value>>())?, |bytes, row| {
            add(bytes, mul(row.entries().len(), size_of::<Value>())?)
        })?;
    let _slots = controls.reserve_query_memory(bytes)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(rows.len())
        .map_err(allocation_error)?;
    #[cfg(test)]
    let mut returning_row_index = 0;
    for row in rows {
        #[cfg(test)]
        {
            super::dml_returning_tests::observe(
                super::dml_returning_tests::Phase::Materializing(returning_row_index),
                controls,
                affected_count,
                std::slice::from_ref(&row),
                1,
            );
            returning_row_index += 1;
        }
        check_timeout(controls)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(row.entries().len())
            .map_err(allocation_error)?;
        values.extend(row.into_entries().into_iter().map(|(_, value)| value));
        output.push(values);
    }
    check_timeout(controls)?;
    Ok(output)
}

fn allocation_error(error: std::collections::TryReserveError) -> QueryError {
    crate::app::CassieError::ResourceLimit(format!(
        "unable to materialize RETURNING result: {error}"
    ))
    .into()
}
