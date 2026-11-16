//! The pattern `WHERE` on selected paths and the `COLUMNS` projection.

use std::mem::size_of;

use super::bindings::{scalar_entries, Candidate};
use super::eval::Evaluator;
use super::plan::Plan;
use super::store::Store;
use super::BatchRow;
use super::QueryError;
use super::SourceExecutionEnv;
use crate::executor::retained_memory::{add, value_clone_bytes};
use crate::runtime::QueryMemoryReservation;

/// One output row per selected path whose pattern `WHERE` is TRUE.
///
/// # Errors
///
/// Expression errors, cancellation and the query memory budget.
pub(super) fn rows(
    env: &SourceExecutionEnv<'_>,
    plan: &Plan<'_>,
    store: &Store,
    eval: &Evaluator<'_>,
    selected: &[Candidate],
    memory: &mut QueryMemoryReservation,
) -> Result<Vec<BatchRow>, QueryError> {
    let table = plan.table;
    // The binder renames references to the output columns onto these positional names.
    let names = (0..table.columns.len())
        .map(crate::sql::table_sources::physical_name)
        .collect::<Vec<_>>();
    let mut out = Vec::new();
    for path in selected {
        env.controls.check_interruption()?;
        let entries = scalar_entries(plan, store, path);
        if let Some(filter) = &table.filter {
            if !eval.holds(filter, entries.clone())? {
                continue;
            }
        }
        let mut values = Vec::with_capacity(names.len());
        let mut bytes = size_of::<BatchRow>() + 64;
        for (name, column) in names.iter().zip(&table.columns) {
            let value = eval.value(&column.expr, entries.clone())?;
            bytes = add(
                bytes,
                add(name.capacity() + 64, value_clone_bytes(&value)?)?,
            )?;
            values.push((name.clone(), value));
        }
        memory.try_grow(bytes)?;
        out.push(BatchRow::new(values));
    }
    Ok(out)
}
