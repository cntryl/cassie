//! `TRUNCATE`: empties every listed table in one statement mutation batch.
//!
//! Rows are removed through the statement's staging session, so a surrounding
//! transaction keeps the truncation private until COMMIT and a ROLLBACK
//! restores the rows. `RESTART IDENTITY` resets the owned sequences; inside a
//! transaction the restart is staged with the rows and publishes at COMMIT.

use std::collections::BTreeSet;

use super::{
    batch, check_timeout, ensure_query_memory_budget, row_id_from_batch_row, scan, Cassie,
    CassieSession, QueryError, QueryExecutionControls, QueryResult,
};
use crate::app::DiagnosticError;
use crate::executor::execution::{command_output, materialized_projection, rollups};
use crate::sql::ast::TruncateStatement;

pub(in crate::executor::execution) fn execute_truncate(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &TruncateStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    check_timeout(controls)?;
    let tables = truncated_tables(cassie, statement)?;
    for table in &tables {
        materialized_projection::reject_write(cassie, table)?;
    }
    let mut collections = BTreeSet::new();
    for table in &tables {
        collections.extend(cassie.referential_write_collections(table));
    }
    let collections = collections.into_iter().collect::<Vec<_>>();
    let output = command_output::empty("TRUNCATE TABLE", controls)?;
    cassie.midge.with_collection_write_gates(&collections, || {
        let batch = cassie
            .new_statement_batch(session)
            .map_err(QueryError::from)?;
        for table in &tables {
            delete_every_row(cassie, batch.session(), table, controls)?;
        }
        check_timeout(controls)?;
        if statement.restart_identity {
            let working_cassie = cassie.with_statement_metadata_writer(batch.session())?;
            restart_owned_sequences(&working_cassie, session, batch.session(), &tables)?;
        }
        cassie
            .publish_statement_batch(session, batch, controls)
            .map_err(QueryError::from)?;
        Ok::<_, QueryError>(())
    })?;
    for table in &tables {
        rollups::mark_source_rollups_stale(cassie, table)?;
    }
    Ok(output)
}

/// The listed tables plus, under `CASCADE`, every table that references one.
fn truncated_tables(
    cassie: &Cassie,
    statement: &TruncateStatement,
) -> Result<Vec<String>, QueryError> {
    let mut tables = Vec::<String>::new();
    for table in &statement.tables {
        let name = canonical_name(cassie, table);
        if !tables
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&name))
        {
            tables.push(name);
        }
    }
    let mut index = 0;
    while index < tables.len() {
        let referenced = tables[index].clone();
        for referencing in referencing_tables(cassie, &referenced) {
            if tables
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&referencing))
            {
                continue;
            }
            if !statement.cascade {
                let child = crate::catalog::local_name(&referencing);
                return Err(QueryError::Cassie(
                    DiagnosticError::new(
                        "0A000",
                        "cannot truncate a table referenced in a foreign key constraint",
                    )
                    .with_detail(format!(
                        "Table \"{child}\" references \"{}\".",
                        crate::catalog::local_name(&referenced)
                    ))
                    .with_hint(format!(
                        "Truncate table \"{child}\" at the same time, or use TRUNCATE ... CASCADE."
                    ))
                    .into(),
                ));
            }
            tables.push(referencing);
        }
        index += 1;
    }
    Ok(tables)
}

fn canonical_name(cassie: &Cassie, table: &str) -> String {
    cassie
        .catalog
        .get_schema(table)
        .map_or_else(|| table.to_string(), |schema| schema.collection)
}

/// Tables whose foreign keys reference `table`, excluding `table` itself.
fn referencing_tables(cassie: &Cassie, table: &str) -> Vec<String> {
    let mut out = Vec::new();
    for collection in cassie.catalog.list_collections_canonical() {
        if collection.name.eq_ignore_ascii_case(table) {
            continue;
        }
        let references =
            cassie
                .catalog
                .get_constraints(&collection.name)
                .iter()
                .any(|constraint| {
                    constraint
                        .references_table
                        .as_deref()
                        .is_some_and(|referenced| {
                            canonical_name(cassie, referenced).eq_ignore_ascii_case(table)
                        })
                });
        if references {
            out.push(collection.name.clone());
        }
    }
    out
}

fn delete_every_row(
    cassie: &Cassie,
    working: &CassieSession,
    table: &str,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    let read_controls = crate::executor::execution::entrypoints::statement_read_controls(
        cassie,
        Some(working),
        controls,
    )?;
    let row_ids = {
        let (_read_scope, _overlay_scope) =
            crate::executor::execution::entrypoints::enter_statement_read(&read_controls);
        let batches = scan::scan(cassie, Some(working), table, &read_controls)?;
        ensure_query_memory_budget(&read_controls, &batches)?;
        batch::flatten_batches(batches)
            .iter()
            .map(row_id_from_batch_row)
            .collect::<Result<Vec<_>, _>>()?
    };
    for row_id in row_ids {
        check_timeout(controls)?;
        cassie
            .delete_document_for_session(Some(working), table, &row_id)
            .map_err(QueryError::from)?;
    }
    Ok(())
}

/// Sequences owned by a column of one of `tables`, including identity sequences.
fn owned_sequences(cassie: &Cassie, tables: &[String]) -> Vec<String> {
    let mut sequences = BTreeSet::new();
    for table in tables {
        for constraint in cassie.catalog.get_constraints(table) {
            if constraint.identity.is_some() {
                if let Some(sequence) = constraint.default_sequence {
                    sequences.insert(sequence);
                }
            }
        }
    }
    for sequence in cassie.catalog.list_sequences() {
        if sequence.owned_by.as_ref().is_some_and(|owner| {
            tables
                .iter()
                .any(|table| canonical_name(cassie, &owner.table).eq_ignore_ascii_case(table))
        }) {
            sequences.insert(sequence.name);
        }
    }
    sequences.into_iter().collect()
}

fn restart_owned_sequences(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    working: &CassieSession,
    tables: &[String],
) -> Result<(), QueryError> {
    // Inside a transaction the restart is staged in the transaction's storage
    // overlay and publishes in the same atomic Data commit as the truncation.
    // Savepoint rollback restores it and ROLLBACK discards it, so there is no
    // separately committed restart to compensate for (#1302).
    let transactional = session.is_some_and(CassieSession::is_transaction_active);
    for sequence in owned_sequences(cassie, tables) {
        let private = cassie
            .catalog
            .get_sequence(&sequence)
            .map(|metadata| cassie.midge.is_private_sequence(&metadata))
            .transpose()
            .map_err(QueryError::from)?
            .unwrap_or(false);
        let staged = private || transactional;
        let _staging_scope = if staged {
            Some(
                cassie
                    .midge
                    .enter_transactional_ddl_scope(
                        working
                            .current_database()
                            .unwrap_or(&cassie.default_database),
                        working.transaction_storage_overlay()?,
                        true,
                    )
                    .map_err(QueryError::from)?,
            )
        } else {
            None
        };
        #[cfg(test)]
        if staged {
            restart_pause::after_overlay_capture();
        }
        let ((), updated) = cassie
            .midge
            .update_sequence(&sequence, |metadata| {
                let mut cursor = metadata.cursor();
                cursor.restart(&metadata.parameters(), None)?;
                metadata.set_cursor(cursor);
                Ok(())
            })
            .map_err(QueryError::from)?;
        if transactional && !private {
            working
                .transaction_storage_overlay()?
                .lock()
                .mark_sequence_restarted(updated.object_id);
        }
        // A staged restart must not reach the published catalog before COMMIT.
        if !staged || working.transaction_catalog_overlay().is_some() {
            cassie.metadata_write_catalog.register_sequence(updated);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "dml_truncate_restart_tests.rs"]
mod restart_tests;

#[cfg(test)]
impl Cassie {
    pub(crate) fn pause_truncate_restart_after_overlay_capture_for_test(
        entered: std::sync::mpsc::SyncSender<()>,
        resume: std::sync::mpsc::Receiver<()>,
    ) -> restart_pause::Guard {
        restart_pause::install(entered, resume)
    }
}

#[cfg(test)]
mod restart_pause {
    use std::cell::RefCell;
    use std::sync::mpsc::{Receiver, SyncSender};
    use std::time::Duration;

    thread_local! {
        static NEXT: RefCell<Option<(SyncSender<()>, Receiver<()>)>> = const { RefCell::new(None) };
    }

    pub(crate) struct Guard;

    impl Drop for Guard {
        fn drop(&mut self) {
            NEXT.with(|slot| {
                slot.borrow_mut().take();
            });
        }
    }

    pub(super) fn install(entered: SyncSender<()>, resume: Receiver<()>) -> Guard {
        NEXT.with(|slot| assert!(slot.replace(Some((entered, resume))).is_none()));
        Guard
    }

    pub(super) fn after_overlay_capture() {
        if let Some((entered, resume)) = NEXT.with(|slot| slot.borrow_mut().take()) {
            entered.send(()).expect("restart overlay captured");
            resume
                .recv_timeout(Duration::from_secs(10))
                .expect("resume restart");
        }
    }
}
