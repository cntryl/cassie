//! Borrow controlled projection state before constructing its owned copies.
use std::mem::size_of;

use crate::app::CassieError;
use crate::executor::batch::{Batch, BatchRow, RowAccess};
use crate::executor::projection::ProjectionOp;
use crate::executor::retained_memory::{
    add, grown_capacity, hash_table_bytes, lookup_bytes, mul, value_clone_bytes,
};
use crate::runtime::QueryExecutionControls;
use crate::sql::SelectItem;
use crate::types::{DataType, Value};

pub(super) fn state_bytes(
    batches: &[Batch],
    projection: &[SelectItem],
    controls: &QueryExecutionControls,
) -> Result<usize, CassieError> {
    if !batches
        .iter()
        .flatten()
        .any(|row| row.operator_memory().is_some())
    {
        return Ok(0);
    }
    let names = projection.iter().try_fold(0, |bytes, item| {
        let name = match item {
            SelectItem::Wildcard => None,
            SelectItem::Column { name, alias } => Some(alias.as_ref().unwrap_or(name)),
            SelectItem::Function { function, alias } => {
                Some(alias.as_ref().unwrap_or(&function.name))
            }
            SelectItem::WindowFunction { function, alias } => {
                Some(alias.as_ref().unwrap_or(&function.name))
            }
            SelectItem::Expr { alias, .. } => alias.as_ref(),
        };
        add(bytes, name.map_or(0, String::len))
    })?;
    let source_names = projection.iter().try_fold(0, |bytes, item| {
        add(
            bytes,
            match item {
                SelectItem::Column { name, .. } => name.len(),
                _ => 0,
            },
        )
    })?;
    // Resolving direct names uses the same parsed lookup authority as projection.
    // Admit its temporary strings before borrowing values from an outer scope.
    let _resolution_scratch = controls.reserve_query_memory(mul(source_names, 64)?)?;
    // The generic path compiles once per batch while its outer op list is live.
    let op_slots = mul(
        grown_capacity(projection.len(), 4)?,
        size_of::<ProjectionOp>(),
    )?;
    let mut bytes = add(
        mul(op_slots, 2)?,
        hash_table_bytes::<String>(projection.len())?,
    )?;
    bytes = add(bytes, add(mul(names, 2)?, mul(source_names, 64)?)?)?;
    let rows = batches
        .iter()
        .try_fold(0, |rows, batch| add(rows, batch.len()))?;
    bytes = add(bytes, mul(rows, 2 * size_of::<BatchRow>())?)?;
    bytes = add(bytes, mul(batches.len(), 2 * size_of::<Batch>())?)?;
    for row in batches.iter().flatten() {
        let width = projection.iter().try_fold(0, |width, item| {
            add(
                width,
                if matches!(item, SelectItem::Wildcard) {
                    row.entries().len()
                } else {
                    1
                },
            )
        })?;
        let slots = grown_capacity(width.max(projection.len()), 4)?;
        bytes = add(bytes, mul(slots, size_of::<(String, Value)>())?)?;
        bytes = add(bytes, lookup_bytes(width, 0)?)?;
        bytes = add(bytes, mul(names, 4)?)?;
        let mut wildcard_names = 0;
        let mut largest_value = 0;
        for (name, value) in row.entries() {
            wildcard_names = add(wildcard_names, name.len())?;
            largest_value = largest_value.max(value_clone_bytes(value)?);
        }
        for item in projection {
            if let SelectItem::Column { name, .. } = item {
                if let Some(value) = row.get(name) {
                    largest_value = largest_value.max(value_clone_bytes(value)?);
                }
            }
        }
        let wildcards = projection
            .iter()
            .filter(|item| matches!(item, SelectItem::Wildcard))
            .count();
        bytes = add(bytes, mul(mul(wildcard_names, wildcards)?, 4)?)?;
        bytes = add(bytes, mul(largest_value, width)?)?;
        if row.has_array_types() {
            let largest_type = row.maximum_type_heap_bytes()?;
            bytes = add(bytes, mul(slots, size_of::<DataType>())?)?;
            bytes = add(bytes, mul(largest_type, width)?)?;
            bytes = add(bytes, size_of::<Vec<DataType>>() + 2 * size_of::<usize>())?;
        }
    }
    Ok(bytes)
}
