//! Direct DISTINCT ON keeps the first occurrence of each key in complete input order.
use std::mem::size_of;
use std::sync::Arc;

use super::{Batch, BatchRow, Expr, QueryError, QueryExecutionControls};
use crate::executor::retained_memory::{add, lookup_bytes, mul};
use crate::executor::typed_batch::{relational::TupleKeys, relational_diagnostics, row_bridge};

pub(super) fn apply(
    batches: Vec<Batch>,
    expressions: &[Expr],
    controls: &QueryExecutionControls,
    scalar: impl FnOnce(Vec<Batch>) -> Result<Vec<Batch>, QueryError>,
) -> Result<Vec<Batch>, QueryError> {
    super::super::check_timeout(controls)?;
    let count = batches
        .iter()
        .try_fold(0, |count, batch| add(count, batch.len()))?;
    if count == 0 {
        relational_diagnostics::publish("distinct_on", "empty_relation");
        return Ok(Vec::new());
    }
    let old_slots = batches.iter().try_fold(
        mul(batches.capacity(), size_of::<Batch>())?,
        |bytes, batch| add(bytes, mul(batch.capacity(), size_of::<BatchRow>())?),
    )?;
    let _flatten_memory =
        controls.reserve_query_memory(add(old_slots, mul(count, size_of::<BatchRow>())?)?)?;
    let mut rows = Vec::with_capacity(count);
    for row in batches.into_iter().flatten() {
        super::super::check_timeout(controls)?;
        rows.push(row);
    }
    let mut memory = super::typed::admit_rows(controls, &rows, &[], count)?;
    memory.try_grow(resolve_bytes(&rows, expressions)?)?;
    let Some(columns) = resolve_columns(&rows, expressions, controls)? else {
        return scalar(vec![rows]);
    };
    let (keys, native) = if let Some(batch) = row_bridge::from_rows(controls, &rows, &columns)? {
        let mapped = (0..columns.len()).collect::<Vec<_>>();
        (TupleKeys::new(controls, &batch, &mapped)?, true)
    } else {
        (TupleKeys::scalar_columns(controls, &rows, &columns)?, false)
    };
    let positions = keys.distinct_positions(controls)?;
    let mut input = rows.into_iter().map(Some).collect::<Vec<_>>();
    let memory = Arc::new(memory);
    let mut selected = Vec::with_capacity(positions.len());
    for position in positions {
        super::super::check_timeout(controls)?;
        let row = input[position]
            .take()
            .expect("distinct positions do not repeat");
        selected.push(row.retain_operator_memory(controls, Arc::clone(&memory))?);
    }
    let mut rows = selected.into_iter();
    let mut output = Vec::with_capacity(
        rows.len()
            .div_ceil(crate::executor::batch::DEFAULT_BATCH_SIZE),
    );
    while rows.len() > 0 {
        super::super::check_timeout(controls)?;
        let count = rows.len().min(crate::executor::batch::DEFAULT_BATCH_SIZE);
        let mut batch = Vec::with_capacity(count);
        batch.extend(rows.by_ref().take(count));
        output.push(batch);
    }
    super::super::check_timeout(controls)?;
    relational_diagnostics::publish(
        "distinct_on",
        if native {
            "native_primitive_keys"
        } else {
            "bounded_semantic_keys"
        },
    );
    Ok(output)
}

fn resolve_bytes(
    rows: &[BatchRow],
    expressions: &[Expr],
) -> Result<usize, crate::app::CassieError> {
    let scratch = expressions.iter().try_fold(0, |bytes, expr| {
        let Expr::Column(name) = expr else {
            unreachable!("direct expressions selected")
        };
        add(bytes, add(512, mul(name.len(), 32)?)?)
    })?;
    rows.iter().try_fold(
        mul(expressions.len(), 2 * size_of::<usize>())?,
        |bytes, row| {
            let names = row
                .entries()
                .iter()
                .map(|(name, _)| name)
                .chain(row.aliases().iter().map(|(name, _)| name))
                .try_fold(0, |bytes, name| add(bytes, name.len()))?;
            add(
                bytes,
                add(
                    scratch,
                    lookup_bytes(add(row.entries().len(), row.aliases().len())?, names)?,
                )?,
            )
        },
    )
}

fn resolve_columns(
    rows: &[BatchRow],
    expressions: &[Expr],
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<usize>>, QueryError> {
    let mut columns = Vec::with_capacity(expressions.len());
    for expr in expressions {
        let Expr::Column(name) = expr else {
            unreachable!("direct expressions selected")
        };
        let mut previous = None;
        for row in rows {
            super::super::check_timeout(controls)?;
            let Some(value) = row.get(name) else {
                return Ok(None);
            };
            let Some(index) = row
                .entries()
                .iter()
                .position(|(_, cell)| std::ptr::eq(cell, value))
            else {
                return Ok(None);
            };
            if previous.is_some_and(|previous| previous != index) {
                return Ok(None);
            }
            previous = Some(index);
        }
        columns.push(previous.expect("nonempty input selected"));
    }
    Ok(Some(columns))
}
