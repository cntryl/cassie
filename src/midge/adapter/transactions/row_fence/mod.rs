//! Old-start row assertions, followed by fresh publication under Cassie's gate.

use std::mem::size_of;
use std::sync::Arc;

use super::{AdapterTransaction, CassieError, Midge, TransactionMode};
use crate::app::TransactionErrorKind;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

pub(crate) struct RetainedRowFence {
    // Native transaction inputs retire before their engine and admission owner.
    transaction: AdapterTransaction,
    midge: Arc<Midge>,
    database: String,
    family_id: cntryl_midge::ColumnFamilyId,
    _memory: QueryMemoryReservation,
}

impl std::fmt::Debug for RetainedRowFence {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RetainedRowFence")
            .finish_non_exhaustive()
    }
}

impl RetainedRowFence {
    pub(crate) fn capture(
        midge: &Arc<Midge>,
        database: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Box<Self>, CassieError> {
        controls.check_interruption()?;
        let bytes = size_of::<Self>()
            .checked_add(database.len())
            .ok_or_else(accounting_overflow)?;
        let memory = controls.reserve_query_memory(bytes)?;
        let family_id = midge.database_family(database)?.id();
        let mut transaction =
            midge.begin_raw_database_adapter_tx(database, TransactionMode::ReadWrite)?;
        transaction.set_conflict_policy(cntryl_midge::ConflictPolicy::LastWriteWins);
        Ok(Box::new(Self {
            transaction,
            midge: Arc::clone(midge),
            database: database.to_owned(),
            family_id,
            _memory: memory,
        }))
    }

    pub(crate) fn validate_rows<'a>(
        self: Box<Self>,
        rows: impl IntoIterator<Item = Result<(&'a str, u64, &'a str), CassieError>>,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        self.validate_rows_observed(rows, controls, || {})
    }

    fn validate_rows_observed<'a>(
        mut self: Box<Self>,
        rows: impl IntoIterator<Item = Result<(&'a str, u64, &'a str), CassieError>>,
        controls: &QueryExecutionControls,
        mut retained: impl FnMut(),
    ) -> Result<(), CassieError> {
        controls.check_interruption()?;
        self.midge.check_storage_recovery_required()?;
        let family = self
            .midge
            .database_family(&self.database)
            .map_err(|_| changed_family())?;
        if family.id() != self.family_id {
            return Err(changed_family());
        }
        let mut inputs = controls.reserve_query_memory(0)?;
        for row in rows {
            controls.check_interruption()?;
            let (collection, relation_id, id) = row?;
            self.assert_row(collection, relation_id, id, controls, &mut inputs)?;
            retained();
        }
        controls.check_interruption()?;
        let options = self.midge.write_options_sync();
        self.transaction
            .commit(options)
            .map_err(|error| match error {
                super::AdapterCommitError::Native(cntryl_midge::MidgeError::WriteConflict(_)) => {
                    CassieError::Transaction {
                        kind: TransactionErrorKind::SerializationFailure,
                        message: "could not serialize access due to concurrent row update"
                            .to_owned(),
                    }
                }
                error => CassieError::from(error),
            })
    }

    fn assert_row(
        &mut self,
        collection: &str,
        relation_id: u64,
        id: &str,
        controls: &QueryExecutionControls,
        inputs: &mut QueryMemoryReservation,
    ) -> Result<(), CassieError> {
        let (keys, scratch) = admitted_row_keys(
            &self.midge,
            &self.database,
            collection,
            relation_id,
            id,
            controls,
        )?;
        let expected = keys.each_ref().map(|key| self.transaction.get(key));
        let [row, column, legacy] = expected;
        let expected = [row?, column?, legacy?];
        // A new logical row is protected by fresh unique/publication checks.
        // An older missing-key assertion would reject a completed insert/delete
        // that PostgreSQL permits before this transaction creates the row.
        if expected.iter().all(Option::is_none) {
            return Ok(());
        }
        // Transfer the already admitted keys without a second reservation.
        inputs.absorb(scratch);
        for (key, expected) in keys.into_iter().zip(expected) {
            controls.check_interruption()?;
            let value_bytes = expected.as_ref().map_or(0, bytes::Bytes::len);
            let bytes = assertion_input_bytes(key.len(), value_bytes)?;
            inputs.try_grow(bytes)?;
            self.transaction
                .assert_value(key, expected.as_deref().map(<[u8]>::to_vec))
                .map_err(CassieError::from)?;
        }
        Ok(())
    }
}

pub(in crate::midge::adapter) fn admitted_row_keys(
    midge: &Midge,
    database: &str,
    collection: &str,
    relation_id: u64,
    id: &str,
    controls: &QueryExecutionControls,
) -> Result<([Vec<u8>; 3], QueryMemoryReservation), CassieError> {
    if relation_id == 0 {
        return Err(CassieError::Unsupported(
            "REPEATABLE READ row validation requires a persisted relation identity".to_owned(),
        ));
    }
    let mut scratch = controls.reserve_query_memory(key_scratch_bytes(
        collection.len(),
        midge.default_database.len(),
        id.len(),
    )?)?;
    let canonical = midge.canonical_collection_name(collection);
    if crate::catalog::relation_database_name(&canonical)
        .is_some_and(|row_database| !row_database.eq_ignore_ascii_case(database))
    {
        return Err(CassieError::Unsupported(
            "REPEATABLE READ cannot publish rows across database families".to_owned(),
        ));
    }
    let keys = [
        Midge::row_key(relation_id, id),
        Midge::column_store_row_key(relation_id, id),
        Midge::doc_key(&canonical, id),
    ];
    drop(canonical);
    let key_bytes = keys.iter().try_fold(0_usize, |bytes, key| {
        bytes
            .checked_add(key.capacity())
            .ok_or_else(accounting_overflow)
    })?;
    scratch.shrink_to(key_bytes);
    Ok((keys, scratch))
}

fn assertion_input_bytes(key_bytes: usize, value_bytes: usize) -> Result<usize, CassieError> {
    // Published 0.5.1 retains an assertion tuple vector, then clones each key
    // into a deduplication HashSet and a KeyAssertion vector during commit.
    // Admit both copies and conservative directory/Bytes-owner backing before
    // the native call. Its independent transaction pool remains in force.
    let directories = 4 * size_of::<(Vec<u8>, Option<Vec<u8>>)>()
        + 4 * (size_of::<Vec<u8>>() + 1)
        + 4 * size_of::<(cntryl_midge::ColumnFamilyId, bytes::Bytes)>()
        + 4 * size_of::<usize>();
    key_bytes
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(value_bytes))
        .and_then(|bytes| bytes.checked_add(directories))
        .ok_or_else(accounting_overflow)
}

fn changed_family() -> CassieError {
    CassieError::Transaction {
        kind: TransactionErrorKind::SerializationFailure,
        message: "database family changed during REPEATABLE READ".to_owned(),
    }
}

fn accounting_overflow() -> CassieError {
    CassieError::ResourceLimit("REPEATABLE READ row assertion size overflow".to_owned())
}

fn key_scratch_bytes(
    collection_bytes: usize,
    database_bytes: usize,
    id_bytes: usize,
) -> Result<usize, CassieError> {
    // Canonical quoting can double input bytes. Bound the current identifier
    // parser's growing String/Vec scratch and all three composite encoders
    // before allocating any key. Actual key capacity replaces this bound once
    // their temporary parser/encoder buffers have retired.
    let name_bytes = collection_bytes
        .checked_add(database_bytes)
        .and_then(|bytes| bytes.checked_add("public".len() + 2))
        .and_then(|bytes| bytes.checked_mul(2))
        .ok_or_else(accounting_overflow)?;
    let parts = name_bytes.checked_add(1).ok_or_else(accounting_overflow)?;
    let string_slots = parts
        .checked_mul(8 * size_of::<String>())
        .ok_or_else(accounting_overflow)?;
    let slice_slots = parts
        .checked_add(6)
        .and_then(|parts| parts.checked_mul(10 * size_of::<&[u8]>()))
        .ok_or_else(accounting_overflow)?;
    // Root(6), baseline(1), longest family(10), encoded relation(9), separators(6).
    let output_bytes = name_bytes
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(id_bytes))
        .and_then(|bytes| bytes.checked_add(32))
        .and_then(|bytes| bytes.checked_mul(3))
        .ok_or_else(accounting_overflow)?;
    let text_buffers = name_bytes
        .checked_mul(4)
        .and_then(|bytes| bytes.checked_add(8 + 2 * 9))
        .ok_or_else(accounting_overflow)?;
    string_slots
        .checked_add(slice_slots)
        .and_then(|bytes| bytes.checked_add(output_bytes))
        .and_then(|bytes| bytes.checked_add(text_buffers))
        .ok_or_else(accounting_overflow)
}

#[cfg(test)]
mod tests;
