//! Selected blocking relational keys; complete input eligibility precedes conversion.
use std::mem::size_of;
use std::sync::Arc;

use super::{Batch, BatchRow, QueryError, QueryExecutionControls, SetOperator};
use crate::executor::retained_memory::{add, lookup_bytes, mul};
use crate::executor::typed_batch::{
    relational::{self, TupleKeys},
    relational_diagnostics, row_bridge,
};
use crate::runtime::QueryMemoryReservation;

pub(super) fn distinct_batches(
    batches: Vec<Batch>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, QueryError> {
    super::super::check_timeout(controls)?;
    let count = batches
        .iter()
        .try_fold(0, |count, batch| add(count, batch.len()))?;
    if count == 0 {
        relational_diagnostics::publish("distinct", "empty_relation");
        return Ok(Vec::new());
    }
    let old_slots = batches.iter().try_fold(
        mul(batches.capacity(), size_of::<Batch>())?,
        |bytes, batch| add(bytes, mul(batch.capacity(), size_of::<BatchRow>())?),
    )?;
    let _flatten_memory =
        controls.reserve_query_memory(add(old_slots, mul(count, size_of::<BatchRow>())?)?)?;
    let mut rows = Vec::with_capacity(count);
    for batch in batches {
        for row in batch {
            super::super::check_timeout(controls)?;
            rows.push(row);
        }
    }
    let rows = distinct(rows, controls)?;
    let mut rows = rows.into_iter();
    let mut output = Vec::with_capacity(
        rows.len()
            .div_ceil(crate::executor::batch::DEFAULT_BATCH_SIZE),
    );
    while rows.len() > 0 {
        let count = rows.len().min(crate::executor::batch::DEFAULT_BATCH_SIZE);
        let mut batch = Vec::with_capacity(count);
        batch.extend(rows.by_ref().take(count));
        output.push(batch);
    }
    super::super::check_timeout(controls)?;
    Ok(output)
}

pub(super) fn distinct(
    rows: Vec<BatchRow>,
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, QueryError> {
    #[cfg(test)]
    relational_diagnostics::input_handoff();
    super::super::check_timeout(controls)?;
    if rows.is_empty() {
        relational_diagnostics::publish("distinct", "empty_relation");
        return Ok(Vec::new());
    }
    let memory = admit_rows(controls, &rows, &[], rows.len())?;
    let (keys, native) = keys(
        controls,
        &rows,
        rows.first().map_or(0, |row| row.entries().len()),
    )?;
    let positions = keys.distinct_positions(controls)?;
    let mut input = rows.into_iter().map(Some).collect::<Vec<_>>();
    let memory = Arc::new(memory);
    let mut output = Vec::with_capacity(positions.len());
    for position in positions {
        super::super::check_timeout(controls)?;
        let row = input[position]
            .take()
            .expect("distinct positions do not repeat");
        output.push(row.retain_operator_memory(controls, Arc::clone(&memory))?);
    }
    super::super::check_timeout(controls)?;
    relational_diagnostics::publish(
        "distinct",
        if native {
            "native_primitive_keys"
        } else {
            "bounded_semantic_keys"
        },
    );
    Ok(output)
}

pub(super) fn set(
    left: Vec<BatchRow>,
    right: Vec<BatchRow>,
    left_names: &[String],
    operator: SetOperator,
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, QueryError> {
    super::super::check_timeout(controls)?;
    let total = add(left.len(), right.len())?;
    if total == 0 {
        relational_diagnostics::publish("set", "empty_relation");
        return Ok(Vec::new());
    }
    let mut memory = admit_rows(controls, &left, &right, total)?;
    memory.try_grow(rekey_bytes(&right, left_names)?)?;
    let right = super::rekey_set_rows(left_names, right);
    let width = left
        .first()
        .or_else(|| right.first())
        .map_or(0, |row| row.entries().len());
    let (left_keys, left_native) = keys(controls, &left, width)?;
    let (right_keys, right_native) = keys(controls, &right, width)?;
    let positions = relational::set_positions(controls, &left_keys, &right_keys, operator)?;
    let mut left = left.into_iter().map(Some).collect::<Vec<_>>();
    let mut right = right.into_iter().map(Some).collect::<Vec<_>>();
    let memory = Arc::new(memory);
    let mut output = Vec::with_capacity(positions.get().len());
    for &(is_right, position) in positions.get() {
        super::super::check_timeout(controls)?;
        let row = if is_right {
            right[position].take()
        } else {
            left[position].take()
        }
        .expect("set positions preserve each input occurrence at most once");
        output.push(row.retain_operator_memory(controls, Arc::clone(&memory))?);
    }
    super::super::check_timeout(controls)?;
    relational_diagnostics::publish(
        "set",
        if left_native && right_native {
            "native_primitive_keys"
        } else {
            "bounded_semantic_keys"
        },
    );
    Ok(output)
}

fn keys(
    controls: &QueryExecutionControls,
    rows: &[BatchRow],
    width: usize,
) -> Result<(TupleKeys, bool), QueryError> {
    let _columns_memory = controls.reserve_query_memory(mul(width, size_of::<usize>())?)?;
    let columns = (0..width).collect::<Vec<_>>();
    if rows.iter().all(|row| row.entries().len() == width) {
        if let Some(batch) = row_bridge::from_rows(controls, rows, &columns)? {
            return TupleKeys::new(controls, &batch, &columns).map(|keys| (keys, true));
        }
    }
    TupleKeys::scalar_rows(controls, rows).map(|keys| (keys, false))
}

fn admit_rows(
    controls: &QueryExecutionControls,
    left: &[BatchRow],
    right: &[BatchRow],
    output_len: usize,
) -> Result<QueryMemoryReservation, QueryError> {
    super::super::check_timeout(controls)?;
    let total = add(left.len(), right.len())?;
    let slots = add(
        mul(total, size_of::<BatchRow>() + size_of::<Option<BatchRow>>())?,
        mul(output_len, size_of::<BatchRow>())?,
    )?;
    let batches = mul(
        output_len.div_ceil(crate::executor::batch::DEFAULT_BATCH_SIZE),
        size_of::<crate::executor::batch::Batch>(),
    )?;
    let bytes = left.iter().chain(right).try_fold(
        add(
            slots,
            add(
                batches,
                size_of::<QueryMemoryReservation>() + 2 * size_of::<usize>(),
            )?,
        )?,
        |bytes, row| add(bytes, row.unleased_body_bytes()?),
    )?;
    controls
        .reserve_query_memory(bytes)
        .map_err(QueryError::from)
}

fn rekey_bytes(rows: &[BatchRow], names: &[String]) -> Result<usize, crate::app::CassieError> {
    if names.is_empty() {
        return Ok(0);
    }
    let name_bytes = names
        .iter()
        .try_fold(0, |bytes, name| add(bytes, name.len()))?;
    let per_row = add(
        mul(names.len(), size_of::<(String, crate::types::Value)>())?,
        add(name_bytes, lookup_bytes(names.len(), name_bytes)?)?,
    )?;
    mul(rows.len(), add(size_of::<BatchRow>(), per_row)?)
}
