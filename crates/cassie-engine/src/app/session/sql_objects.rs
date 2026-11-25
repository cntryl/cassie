//! Session-owned prepared statements and cursors.
//!
//! SQL `PREPARE` and protocol-level Parse share one namespace, as in
//! PostgreSQL: protocol statements are registered here by name so that
//! `DEALLOCATE`, `DISCARD ALL` and `pg_prepared_statements` see them.
//! Cursors are materialized when declared and follow PostgreSQL's
//! transaction rules: a cursor without `WITH HOLD` closes when its
//! transaction ends, and any cursor declared in a transaction that rolls
//! back (or after a savepoint that is rolled back to) is closed.

use std::collections::BTreeMap;

use super::{CassieError, CassieSession, FailureAuthorityIdentity, SessionTransactionStatus};
use crate::executor::QueryResult;
use std::sync::Arc;

/// Most cursors one session may hold open at once.
pub(crate) const MAX_CURSORS_PER_SESSION: usize = 256;
/// Most SQL and protocol prepared statements one session may register.
pub(crate) const MAX_PREPARED_PER_SESSION: usize = 1024;

#[derive(Debug, Clone)]
pub(crate) struct SessionPrepared {
    /// The text `pg_prepared_statements.statement` shows.
    pub(crate) statement_text: String,
    /// The statement body run by `EXECUTE`.
    pub(crate) body_sql: String,
    /// Declared parameter types, as written; empty for an undeclared slot.
    pub(crate) parameter_types: Vec<String>,
    pub(crate) result_types: Vec<String>,
    /// `None` for SQL `PREPARE`; the connection's statement id otherwise.
    pub(crate) wire_id: Option<u64>,
    pub(crate) prepare_time: i64,
}

/// The `DECLARE` options `pg_cursors` reports.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CursorOptions {
    pub(crate) scroll: bool,
    pub(crate) hold: bool,
    pub(crate) binary: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionCursor {
    pub(crate) statement_text: String,
    pub(crate) result: QueryResult,
    /// 0 is before the first row; `rows + 1` is after the last.
    pub(crate) position: usize,
    pub(crate) options: CursorOptions,
    pub(crate) creation_time: i64,
    /// Declared in the open transaction, so its rollback closes the cursor.
    pub(crate) in_transaction: bool,
    pub(crate) sequence: u64,
}

#[derive(Debug, Default)]
pub(crate) struct SessionObjects {
    prepared: BTreeMap<String, SessionPrepared>,
    cursors: BTreeMap<String, SessionCursor>,
    next_cursor_sequence: u64,
    /// Bumped whenever SQL removes protocol-level statements.
    wire_epoch: u64,
}

impl SessionObjects {
    pub(crate) fn prepared(&self, name: &str) -> Option<&SessionPrepared> {
        self.prepared.get(name)
    }

    pub(crate) fn prepared_statements(&self) -> impl Iterator<Item = (&String, &SessionPrepared)> {
        self.prepared.iter()
    }

    pub(crate) fn prepared_count(&self) -> usize {
        self.prepared.len()
    }

    pub(crate) fn insert_prepared(&mut self, name: String, prepared: SessionPrepared) {
        self.prepared.insert(name, prepared);
    }

    /// Removes one statement; a protocol statement's removal is published
    /// to its connection through the wire epoch.
    pub(crate) fn remove_prepared(&mut self, name: &str) -> Option<SessionPrepared> {
        let removed = self.prepared.remove(name)?;
        if removed.wire_id.is_some() {
            self.wire_epoch = self.wire_epoch.wrapping_add(1);
        }
        Some(removed)
    }

    pub(crate) fn clear_prepared(&mut self) {
        if self
            .prepared
            .values()
            .any(|prepared| prepared.wire_id.is_some())
        {
            self.wire_epoch = self.wire_epoch.wrapping_add(1);
        }
        self.prepared.clear();
    }

    pub(crate) fn cursor(&self, name: &str) -> Option<&SessionCursor> {
        self.cursors.get(name)
    }

    pub(crate) fn cursor_mut(&mut self, name: &str) -> Option<&mut SessionCursor> {
        self.cursors.get_mut(name)
    }

    pub(crate) fn cursors(&self) -> impl Iterator<Item = (&String, &SessionCursor)> {
        self.cursors.iter()
    }

    pub(crate) fn cursor_count(&self) -> usize {
        self.cursors.len()
    }

    pub(crate) fn insert_cursor(&mut self, name: String, mut cursor: SessionCursor) {
        cursor.sequence = self.next_cursor_sequence;
        self.next_cursor_sequence = self.next_cursor_sequence.wrapping_add(1);
        self.cursors.insert(name, cursor);
    }

    pub(crate) fn remove_cursor(&mut self, name: &str) -> Option<SessionCursor> {
        self.cursors.remove(name)
    }

    pub(crate) fn clear_cursors(&mut self) {
        self.cursors.clear();
    }

    pub(crate) const fn cursor_mark(&self) -> u64 {
        self.next_cursor_sequence
    }

    fn end_transaction(&mut self, committed: bool) {
        self.cursors
            .retain(|_, cursor| cursor.options.hold && (committed || !cursor.in_transaction));
        for cursor in self.cursors.values_mut() {
            cursor.in_transaction = false;
        }
    }

    fn rollback_to(&mut self, mark: u64) {
        self.cursors
            .retain(|_, cursor| !cursor.in_transaction || cursor.sequence < mark);
    }
}

impl CassieSession {
    /// Only bounded in-memory leaves run here; scoped forks validate their real root.
    pub(crate) fn with_owned_shared_session_state<T>(
        &self,
        operation: impl FnOnce(SessionTransactionStatus, bool) -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        self.with_owned_shared_transaction_inner(true, |transaction| {
            operation(transaction.status, transaction.implicit)
        })
    }

    /// Clock callers receive only a guarded status/generation view. An owned
    /// retired root may build Idle statement time, but never publish a clock.
    pub(super) fn with_owned_temporal_transaction<T>(
        &self,
        operation: impl FnOnce(SessionTransactionStatus, u64) -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        self.with_owned_shared_transaction_inner(true, |transaction| {
            operation(transaction.status, transaction.generation)
        })
    }

    pub(super) fn with_owned_setting_transaction<T>(
        &self,
        operation: impl FnOnce(&mut super::SessionTransactionState) -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        if self.operation_failure_authority.is_none() && self.owns_transaction_lifecycle {
            let mut transaction = self.transaction.lock();
            return operation(&mut transaction);
        }
        self.with_owned_shared_transaction_inner(false, operation)
    }

    pub(super) fn with_owned_setting_state<T>(
        &self,
        operation: impl FnOnce() -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        self.with_owned_setting_transaction(|_| operation())
    }

    fn with_owned_shared_transaction_inner<T>(
        &self,
        allow_owned_retirement: bool,
        operation: impl FnOnce(&mut super::SessionTransactionState) -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        let authority = self.operation_failure_authority.as_ref();
        let root = if let Some(authority) = authority {
            if self.owns_transaction_lifecycle && !authority.matches_root(self) {
                return Err(super::sequence_effect::changed());
            }
            authority
                .root
                .upgrade()
                .ok_or_else(super::sequence_effect::changed)?
        } else if self.owns_transaction_lifecycle {
            Arc::clone(&self.transaction)
        } else {
            return Err(super::sequence_effect::changed());
        };
        let mut transaction = root.lock();
        if let Some(authority) = authority {
            if authority.identity()
                != Some(FailureAuthorityIdentity::from_transaction(&transaction))
            {
                let owner = authority.owner.lock();
                let owned_idle_retirement = allow_owned_retirement
                    && self.owns_transaction_lifecycle
                    && transaction.status == SessionTransactionStatus::Idle
                    && owner.captured
                    && !owner.admission_rejected
                    && owner.retired_generation == Some(transaction.generation)
                    && owner.generation == transaction.generation;
                if !owned_idle_retirement {
                    return Err(super::sequence_effect::changed());
                }
            }
            if !Arc::ptr_eq(&root, &self.transaction)
                && !Arc::ptr_eq(
                    &transaction.projection_write,
                    &self.transaction.lock().projection_write,
                )
            {
                return Err(super::sequence_effect::changed());
            }
        }
        if transaction.sealed_for_commit
            && (!self.owns_transaction_lifecycle || !transaction.may_be_failed_by_current_thread())
        {
            return Err(super::sequence_effect::changed());
        }
        operation(&mut transaction)
    }

    pub(crate) fn with_owned_session_objects<T>(
        &self,
        operation: impl FnOnce(
            &mut SessionObjects,
            SessionTransactionStatus,
            bool,
        ) -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        self.with_owned_shared_session_state(|status, implicit| {
            if status == SessionTransactionStatus::Failed {
                return Err(CassieError::failed_transaction());
            }
            operation(&mut self.session_objects.lock(), status, implicit)
        })
    }

    pub(crate) fn validate_discard_all_owner(&self) -> Result<(), CassieError> {
        self.with_owned_shared_session_state(Self::validate_discard_all_state)
    }

    fn validate_discard_all_state(
        status: SessionTransactionStatus,
        implicit: bool,
    ) -> Result<(), CassieError> {
        if status == SessionTransactionStatus::Failed {
            return Err(CassieError::failed_transaction());
        }
        if status == SessionTransactionStatus::InTransaction && !implicit {
            return Err(CassieError::Transaction {
                kind: crate::app::TransactionErrorKind::ActiveTransaction,
                message: "DISCARD ALL cannot run inside a transaction block".to_owned(),
            });
        }
        Ok(())
    }

    pub(crate) fn discard_all_owned_state(&self) -> Result<(), CassieError> {
        self.with_owned_sequence_reset(|status, implicit| {
            Self::validate_discard_all_state(status, implicit)?;
            let mut objects = self.session_objects.lock();
            objects.clear_cursors();
            objects.clear_prepared();
            let mut settings = self.settings.lock();
            settings.reset_all();
            let search_path = settings
                .get_ref("search_path")
                .expect("registered search_path")
                .to_owned();
            drop(settings);
            self.set_search_path_locked(
                search_path
                    .split(',')
                    .map(|name| name.trim().to_owned())
                    .collect(),
            );
            Ok(())
        })
    }

    pub(crate) fn with_session_objects<T>(
        &self,
        operation: impl FnOnce(&mut SessionObjects) -> T,
    ) -> T {
        operation(&mut self.session_objects.lock())
    }

    /// The epoch a connection compares to drop protocol statements that SQL
    /// deallocated.
    #[doc(hidden)]
    #[must_use]
    pub fn prepared_wire_epoch(&self) -> u64 {
        self.session_objects.lock().wire_epoch
    }

    /// Whether protocol statement `name` with connection id `id` is still
    /// registered.
    #[doc(hidden)]
    #[must_use]
    pub fn has_wire_prepared(&self, name: &str, id: u64) -> bool {
        self.session_objects
            .lock()
            .prepared
            .get(name)
            .is_some_and(|prepared| prepared.wire_id == Some(id))
    }

    #[cfg(any(test, feature = "transport-test-hooks"))]
    #[doc(hidden)]
    #[must_use]
    pub fn has_prepared_for_test(&self, name: &str) -> bool {
        self.session_objects.lock().prepared.contains_key(name)
    }

    /// Whether `name` is held by a SQL-level `PREPARE`.
    #[doc(hidden)]
    #[must_use]
    pub fn has_sql_prepared(&self, name: &str) -> bool {
        self.session_objects
            .lock()
            .prepared
            .get(name)
            .is_some_and(|prepared| prepared.wire_id.is_none())
    }

    /// Registers a named protocol-level statement in the shared namespace.
    #[doc(hidden)]
    pub fn register_wire_prepared(
        &self,
        name: &str,
        id: u64,
        query: &str,
    ) -> Result<(), CassieError> {
        if name.is_empty() {
            return Ok(());
        }
        let prepared = SessionPrepared {
            statement_text: query.to_string(),
            body_sql: query.to_string(),
            parameter_types: vec![
                String::new();
                crate::sql::parser::max_positional_parameter(query)
            ],
            result_types: Vec::new(),
            wire_id: Some(id),
            prepare_time: crate::types::extended::temporal::session::current_timestamp(),
        };
        if self.operation_failure_authority.is_none() && self.owns_transaction_lifecycle {
            self.session_objects
                .lock()
                .prepared
                .insert(name.to_owned(), prepared);
            return Ok(());
        }
        self.with_owned_shared_session_state(|_, _| {
            let mut objects = self.session_objects.lock();
            if objects.prepared(name).is_some() {
                return Err(crate::app::SessionObjectError::duplicate_prepared(name).into());
            }
            if objects.prepared_count() >= MAX_PREPARED_PER_SESSION {
                return Err(CassieError::ResourceLimit(
                    "prepared statement limit exceeded".to_owned(),
                ));
            }
            objects.insert_prepared(name.to_owned(), prepared);
            Ok(())
        })
    }

    /// Forgets a protocol-level statement its connection closed.
    #[doc(hidden)]
    pub fn forget_wire_prepared(&self, name: &str, id: u64) -> Result<(), CassieError> {
        let forget = |objects: &mut SessionObjects| {
            if objects
                .prepared
                .get(name)
                .is_some_and(|prepared| prepared.wire_id == Some(id))
            {
                objects.prepared.remove(name);
            }
        };
        if self.operation_failure_authority.is_none() && self.owns_transaction_lifecycle {
            forget(&mut self.session_objects.lock());
            return Ok(());
        }
        self.with_owned_shared_session_state(|_, _| {
            forget(&mut self.session_objects.lock());
            Ok(())
        })
    }

    pub(super) fn end_transaction_objects(&self, committed: bool) {
        self.session_objects.lock().end_transaction(committed);
    }

    pub(super) fn session_objects_mark(&self) -> u64 {
        self.session_objects.lock().cursor_mark()
    }

    pub(super) fn rollback_session_objects_to(&self, mark: u64) {
        self.session_objects.lock().rollback_to(mark);
    }
}
