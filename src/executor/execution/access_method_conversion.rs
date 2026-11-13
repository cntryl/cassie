//! `ALTER TABLE ... SET ACCESS METHOD heap | columnar`.
//!
//! Conversion rewrites the table's authoritative rows in one Midge
//! transaction (see `Midge::convert_collection_storage_mode`). The rewrite is
//! bounded by the query memory budget, so an oversized table fails with
//! `54000` before anything is written.
use std::cell::RefCell;

use super::{Cassie, QueryError};
use crate::app::CassieError;
use crate::catalog::collections::CollectionStorageMode;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

pub(super) fn set_access_method(
    cassie: &Cassie,
    table: &str,
    access_method: &str,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    let target = match access_method {
        "heap" => CollectionStorageMode::RowStore,
        "columnar" => CollectionStorageMode::ColumnStore,
        other => {
            return Err(CassieError::Unsupported(format!(
                "unsupported ALTER TABLE access method '{other}'"
            ))
            .into());
        }
    };
    let current = cassie
        .catalog
        .collection_storage_mode(table)
        .ok_or_else(|| CassieError::CollectionNotFound(table.to_owned()))?;
    if current.uses_column_store_storage() == target.uses_column_store_storage() {
        return Ok(());
    }
    if target.uses_column_store_storage() {
        reject_column_accelerator(cassie, table)?;
    }
    controls.check_interruption()?;

    let reservations = RefCell::new(Vec::<QueryMemoryReservation>::new());
    let converted = cassie
        .midge
        .with_collection_gates(&[table.to_owned()], || {
            let mut charge = |bytes: usize| -> Result<(), CassieError> {
                controls.check_interruption()?;
                reservations
                    .borrow_mut()
                    .push(controls.reserve_query_memory(bytes)?);
                Ok(())
            };
            cassie
                .midge
                .convert_collection_storage_mode(table, target, &mut charge)
        })?;
    if converted.is_some() {
        cassie.catalog.set_collection_storage_mode(table, target);
        cassie.bump_schema_epoch_and_invalidate_query_cache()?;
    }
    drop(reservations);
    Ok(())
}

/// A `USING column` index is a derived layout over a row table and cannot
/// coexist with column authority; it must be dropped first.
fn reject_column_accelerator(cassie: &Cassie, table: &str) -> Result<(), QueryError> {
    let blocking = cassie
        .catalog
        .list_indexes(table)
        .into_iter()
        .find(|index| index.kind == crate::catalog::IndexKind::Column);
    if let Some(index) = blocking {
        return Err(CassieError::Unsupported(format!(
            "cannot convert '{table}' to columnar: drop column index '{}' first",
            index.name
        ))
        .into());
    }
    Ok(())
}
