//! Admit new qualification state while retaining every existing row origin.
use std::mem::size_of;
use std::sync::Arc;

use crate::executor::retained_memory::add;
use crate::runtime::QueryMemoryReservation;

use super::{
    check_timeout, qualify_batches, qualify_row, source_collection, Batch, BatchRow, QueryError,
    SourceExecutionEnv,
};

pub(super) fn qualify_owned(
    env: &SourceExecutionEnv<'_>,
    mut batches: Vec<Batch>,
    qualifier: &str,
) -> Result<Vec<Batch>, QueryError> {
    let owned = |row: &BatchRow| row.query_memory().is_some() || row.operator_memory().is_some();
    if !batches.iter().flatten().any(owned) {
        return Ok(qualify_batches(batches, qualifier));
    }
    check_timeout(env.controls)?;
    let bytes = batches.iter().flatten().filter(|row| owned(row)).try_fold(
        size_of::<QueryMemoryReservation>() + 2 * size_of::<usize>(),
        |bytes, row| {
            add(
                bytes,
                source_collection::row_qualification_bytes(qualifier, row)?,
            )
        },
    )?;
    let memory = Arc::new(env.controls.reserve_query_memory(bytes)?);
    let _scratch = source_collection::qualification_scratch(env, qualifier)?;
    for row in batches.iter_mut().flatten() {
        check_timeout(env.controls)?;
        let retain = owned(row);
        let previous = std::mem::replace(row, BatchRow::from_projected_values(Vec::new()));
        *row = qualify_row(previous, qualifier);
        if retain {
            row.attach_operator_memory(env.controls, Arc::clone(&memory))?;
        }
    }
    check_timeout(env.controls)?;
    Ok(batches)
}

/// Admit temporary parsing, hexadecimal encoding and formatted carrier backing.
pub(super) fn alias_qualifier(
    env: &SourceExecutionEnv<'_>,
    alias: &str,
) -> Result<(String, QueryMemoryReservation), QueryError> {
    check_timeout(env.controls)?;
    let scratch = source_collection::qualification_scratch(env, alias)?;
    Ok((crate::sql::binder::alias_row_qualifier(alias), scratch))
}
