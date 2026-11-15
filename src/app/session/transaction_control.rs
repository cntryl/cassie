//! Session transaction characteristics and ownership boundaries.

use super::{
    Arc, CassieError, CassieSession, Mutex, SessionTransactionStatus, SharedTransactionWrites,
    TransactionIsolation,
};
use crate::app::TransactionErrorKind;
use crate::catalog::Catalog;
use crate::sql::ast::TransactionModes;

impl CassieSession {
    /// Observe root status under the transaction mutex. Detached forks have
    /// their own mutex and must not be mistaken for the root's authority.
    pub(crate) fn with_root_transaction_status_locked<T>(
        &self,
        operation: impl FnOnce(Option<SessionTransactionStatus>) -> T,
    ) -> T {
        if !self.owns_transaction_lifecycle {
            return operation(None);
        }
        let transaction = self.transaction.lock();
        operation(Some(transaction.status))
    }

    pub(crate) fn transaction_relation_exists(&self, catalog: &Catalog, relation: &str) -> bool {
        let transaction = self.transaction.lock();
        transaction.catalog_overlay.as_ref().map_or_else(
            || catalog.relation_exists(relation),
            |overlay| overlay.relation_exists(relation),
        )
    }

    #[cfg(test)]
    pub(crate) fn begin_transaction(
        &self,
        isolation: Option<TransactionIsolation>,
    ) -> Result<(), CassieError> {
        self.begin_transaction_with_modes(isolation, TransactionModes::default())
    }

    #[cfg(test)]
    pub(crate) fn begin_transaction_with_modes(
        &self,
        isolation: Option<TransactionIsolation>,
        modes: TransactionModes,
    ) -> Result<(), CassieError> {
        self.begin_transaction_with_origin(isolation, modes, false)
    }

    pub(crate) fn begin_transaction_with_modes_for_operation(
        &self,
        isolation: Option<TransactionIsolation>,
        modes: TransactionModes,
        authority: Option<&super::FailureAuthority>,
    ) -> Result<(), CassieError> {
        self.begin_transaction_with_origin_for_operation(isolation, modes, false, authority)
    }

    #[cfg(test)]
    pub(super) fn begin_transaction_with_origin(
        &self,
        isolation: Option<TransactionIsolation>,
        modes: TransactionModes,
        implicit: bool,
    ) -> Result<(), CassieError> {
        self.begin_transaction_with_origin_for_operation(isolation, modes, implicit, None)
    }

    pub(super) fn begin_transaction_with_origin_for_operation(
        &self,
        isolation: Option<TransactionIsolation>,
        modes: TransactionModes,
        implicit: bool,
        authority: Option<&super::FailureAuthority>,
    ) -> Result<(), CassieError> {
        if isolation == Some(TransactionIsolation::Serializable) {
            return Err(CassieError::Unsupported(
                "SERIALIZABLE transaction isolation is unsupported".to_string(),
            ));
        }
        let requested_modes = modes;
        let modes = TransactionModes {
            read_only: Some(
                modes
                    .read_only
                    .unwrap_or_else(|| self.default_transaction_read_only()),
            ),
            deferrable: Some(
                modes
                    .deferrable
                    .unwrap_or_else(|| self.default_transaction_deferrable()),
            ),
        };
        if isolation == Some(TransactionIsolation::RepeatableRead) && modes.deferrable == Some(true)
        {
            return Err(CassieError::Unsupported(
                "DEFERRABLE REPEATABLE READ is unsupported".to_owned(),
            ));
        }
        let mut transaction = self.transaction.lock();
        // Snapshot under the transaction mutex. Retirement restores session
        // state under the same mutex, so a new transaction never captures the
        // previous transaction's `SET LOCAL` values.
        let session_state = self.session_state_snapshot();
        if transaction.status == SessionTransactionStatus::Failed
            || (transaction.status != SessionTransactionStatus::Idle && !transaction.implicit)
        {
            return Err(CassieError::Unsupported(
                "transaction already in progress".to_string(),
            ));
        }
        if transaction.implicit {
            if transaction.has_query {
                let current_isolation = transaction
                    .isolation
                    .unwrap_or(TransactionIsolation::ReadCommitted);
                let changed_isolation =
                    isolation.is_some_and(|isolation| isolation != current_isolation);
                let changed_deferrable = requested_modes
                    .deferrable
                    .is_some_and(|value| Some(value) != transaction.modes.deferrable);
                let changed_read_write = requested_modes.read_only == Some(false)
                    && transaction.modes.read_only == Some(true);
                if changed_isolation || changed_deferrable || changed_read_write {
                    return Err(CassieError::Transaction {
                        kind: TransactionErrorKind::ActiveTransaction,
                        message: "transaction characteristics must be set before any query"
                            .to_string(),
                    });
                }
            }
            transaction.implicit = implicit;
            transaction.isolation = isolation.or(transaction.isolation);
            transaction.modes.read_only = requested_modes.read_only.or(transaction.modes.read_only);
            transaction.modes.deferrable =
                requested_modes.deferrable.or(transaction.modes.deferrable);
            return Ok(());
        }
        transaction.status = SessionTransactionStatus::InTransaction;
        transaction.implicit = implicit;
        transaction.isolation = isolation;
        transaction.modes = modes;
        transaction.has_query = false;
        transaction.generation = transaction.generation.wrapping_add(1);
        transaction.rollback_epoch = 0;
        transaction.unique_owner_id = uuid::Uuid::new_v4();
        transaction.writes = SharedTransactionWrites::default();
        transaction.conflict_intents.clear();
        transaction.savepoints.clear();
        transaction.repeatable_read = None;
        transaction.catalog_overlay = None;
        transaction.catalog_base_version = None;
        transaction.catalog_views = super::catalog_overlay::TransactionCatalogViews::default();
        transaction.storage_overlay =
            Arc::new(Mutex::new(crate::midge::adapter::StorageOverlay::default()));
        transaction.session_state = Some(session_state);
        transaction.local_settings.clear();
        transaction.projection_write = Arc::default();
        transaction.sealed_for_commit = false;
        transaction.commit_thread = None;
        if let Some(authority) = authority {
            authority.adopt_start(self, &transaction);
        }
        Ok(())
    }

    pub(crate) fn set_transaction_isolation(&self, value: &str) -> Result<(), CassieError> {
        let mut transaction = self.transaction.lock();
        if transaction.status == SessionTransactionStatus::Idle || transaction.implicit {
            // The bounded #819 profile keeps this a deterministic 0A000 rejection;
            // PostgreSQL's 25P01 warning-and-no-op form is not selected.
            return Err(CassieError::Unsupported(
                "SET TRANSACTION requires an explicit transaction block".to_owned(),
            ));
        }
        if transaction.status == SessionTransactionStatus::Failed {
            return Err(CassieError::failed_transaction());
        }
        if transaction.has_query {
            return Err(CassieError::Transaction {
                kind: TransactionErrorKind::ActiveTransaction,
                message: "SET TRANSACTION ISOLATION LEVEL must be called before any query"
                    .to_owned(),
            });
        }
        if !transaction.savepoints.is_empty() {
            return Err(CassieError::Transaction {
                kind: TransactionErrorKind::ActiveTransaction,
                message: "SET TRANSACTION ISOLATION LEVEL must not be called in a subtransaction"
                    .to_owned(),
            });
        }

        let isolation = match value.trim().to_ascii_lowercase().as_str() {
            "read committed" => TransactionIsolation::ReadCommitted,
            "read uncommitted" => TransactionIsolation::ReadUncommitted,
            "repeatable read" => TransactionIsolation::RepeatableRead,
            "serializable" => {
                return Err(CassieError::Unsupported(
                    "SERIALIZABLE transaction isolation is unsupported".to_owned(),
                ));
            }
            _ => {
                return Err(CassieError::Unsupported(
                    "unsupported transaction isolation level".to_owned(),
                ));
            }
        };
        if isolation == TransactionIsolation::RepeatableRead
            && transaction.modes.deferrable == Some(true)
        {
            return Err(CassieError::Unsupported(
                "DEFERRABLE REPEATABLE READ is unsupported".to_owned(),
            ));
        }
        transaction.isolation = Some(isolation);
        Ok(())
    }

    pub(crate) fn transaction_characteristics(
        &self,
    ) -> (Option<TransactionIsolation>, TransactionModes) {
        let transaction = self.transaction.lock();
        (transaction.isolation, transaction.modes)
    }

    pub(crate) fn transaction_generation(&self) -> u64 {
        self.transaction.lock().generation
    }

    pub(crate) fn mark_transaction_query(&self) {
        let mut transaction = self.transaction.lock();
        if transaction.status == SessionTransactionStatus::InTransaction {
            transaction.has_query = true;
        }
    }

    pub(crate) fn ensure_transaction_writable(&self) -> Result<(), CassieError> {
        self.ensure_transaction_writable_inner(|| {})
    }

    #[cfg(test)]
    fn ensure_transaction_writable_observed(
        &self,
        after_decision: impl FnOnce(),
    ) -> Result<(), CassieError> {
        self.ensure_transaction_writable_inner(after_decision)
    }

    fn ensure_transaction_writable_inner(
        &self,
        after_decision: impl FnOnce(),
    ) -> Result<(), CassieError> {
        let read_only = {
            let mut transaction = self.transaction.lock();
            if transaction.status == SessionTransactionStatus::Idle {
                // Settings have their own lock; an idle check must not later
                // fail a transaction started by another session clone.
                drop(transaction);
                self.default_transaction_read_only()
            } else {
                let read_only = transaction.modes.read_only.unwrap_or(false);
                // Make the decision and failure transition against one owner.
                if read_only && transaction.status == SessionTransactionStatus::InTransaction {
                    transaction.status = SessionTransactionStatus::Failed;
                }
                read_only
            }
        };
        after_decision();
        if read_only {
            return Err(CassieError::Transaction {
                kind: TransactionErrorKind::ReadOnlyTransaction,
                message: "cannot execute a write in a read-only transaction".to_string(),
            });
        }
        Ok(())
    }

    pub(super) fn default_transaction_read_only(&self) -> bool {
        self.settings
            .lock()
            .get_ref("default_transaction_read_only")
            .is_ok_and(|value| value == "on")
    }

    pub(super) fn default_transaction_deferrable(&self) -> bool {
        self.settings
            .lock()
            .get_ref("default_transaction_deferrable")
            .is_ok_and(|value| value == "on")
    }

    pub(super) fn set_transaction_read_only(&self, value: &str) -> Result<String, CassieError> {
        let read_only = crate::app::session_settings::parse_bool(value)?;
        let mut transaction = self.transaction.lock();
        if !read_only
            && transaction.modes.read_only == Some(true)
            && (transaction.has_query
                || transaction
                    .savepoints
                    .iter()
                    .any(|savepoint| savepoint.modes.read_only == Some(true)))
        {
            return Err(CassieError::Transaction {
                kind: TransactionErrorKind::ActiveTransaction,
                message: "transaction read-write mode must be set before any query".to_string(),
            });
        }
        transaction.modes.read_only = Some(read_only);
        Ok(if read_only { "on" } else { "off" }.to_string())
    }

    pub(crate) fn transaction_setting(&self, name: &str) -> Option<&'static str> {
        if super::is_projection_write_mode(name) {
            return Some(self.projection_write_mode().as_str());
        }
        let transaction = self.transaction.lock();
        match name.trim().to_ascii_lowercase().as_str() {
            "transaction_isolation" | "transaction isolation level" => Some(
                match transaction
                    .isolation
                    .unwrap_or(TransactionIsolation::ReadCommitted)
                {
                    TransactionIsolation::ReadUncommitted => "read uncommitted",
                    TransactionIsolation::ReadCommitted => "read committed",
                    TransactionIsolation::RepeatableRead => "repeatable read",
                    TransactionIsolation::Serializable => "serializable",
                },
            ),
            "transaction_read_only" => {
                let read_only = if transaction.status == SessionTransactionStatus::Idle {
                    drop(transaction);
                    self.default_transaction_read_only()
                } else {
                    transaction.modes.read_only.unwrap_or(false)
                };
                Some(if read_only { "on" } else { "off" })
            }
            "transaction_deferrable" => {
                let deferrable = if transaction.status == SessionTransactionStatus::Idle {
                    drop(transaction);
                    self.default_transaction_deferrable()
                } else {
                    transaction.modes.deferrable.unwrap_or(false)
                };
                Some(if deferrable { "on" } else { "off" })
            }
            _ => None,
        }
    }

    pub(crate) fn commit_transaction(&self) {
        let transaction = self.transaction.lock();
        self.retire_committed_locked(transaction);
    }

    /// Whether a COMMIT currently owns this session's transaction.
    #[cfg(test)]
    pub(crate) fn is_sealed_for_commit(&self) -> bool {
        self.transaction.lock().sealed_for_commit
    }

    /// Whether a COMMIT running on another thread owns the transaction. The
    /// committing thread itself is never refused its own transaction control.
    pub(crate) fn is_committing_on_another_thread(&self) -> bool {
        let transaction = self.transaction.lock();
        transaction.sealed_for_commit
            && transaction.commit_thread != Some(std::thread::current().id())
    }

    fn retire_committed_locked(
        &self,
        mut transaction: parking_lot::MutexGuard<'_, super::SessionTransactionState>,
    ) {
        let unique_owner_id = transaction.unique_owner_id;
        transaction.status = SessionTransactionStatus::Idle;
        transaction.implicit = false;
        transaction.isolation = None;
        transaction.modes = TransactionModes::default();
        transaction.has_query = false;
        transaction.unique_owner_id = uuid::Uuid::new_v4();
        transaction.writes = SharedTransactionWrites::default();
        transaction.conflict_intents.clear();
        transaction.savepoints.clear();
        transaction.repeatable_read = None;
        transaction.catalog_overlay = None;
        transaction.catalog_base_version = None;
        transaction.catalog_views = super::catalog_overlay::TransactionCatalogViews::default();
        transaction.storage_overlay =
            Arc::new(Mutex::new(crate::midge::adapter::StorageOverlay::default()));
        let session_state = transaction.session_state.take();
        let locals = std::mem::take(&mut transaction.local_settings);
        transaction.projection_write = Arc::default();
        transaction.sealed_for_commit = false;
        transaction.commit_thread = None;
        // Retire everything the transaction owned before releasing the mutex.
        // BEGIN needs this mutex, so no later transaction can start, attach a
        // schema context, declare a cursor or snapshot settings until the old
        // transaction's cleanup is complete (#1290). These are in-memory
        // operations; no storage work runs under the mutex.
        self.retire_schema_context();
        if let Some(waits) = self.transaction_unique_waits() {
            waits.release_owner(unique_owner_id);
        }
        self.end_transaction_objects(true);
        if let Some(state) = session_state {
            let mut settings = self.settings.lock();
            for name in locals {
                settings.restore_from(&name, &state.settings);
            }
        }
        #[cfg(test)]
        retirement_tests::observe_retirement(self);
        drop(transaction);
    }

    /// Seals the active transaction for COMMIT. From here its captured writes
    /// and declared projection write mode cannot change through a statement
    /// fork or a new declaration. The seal is released when the returned guard
    /// drops, which matters only if COMMIT fails and leaves the transaction open.
    pub(crate) fn seal_transaction_for_commit(&self) -> CommitSeal<'_> {
        let mut transaction = self.transaction.lock();
        let sealed = transaction.status == SessionTransactionStatus::InTransaction
            && !transaction.sealed_for_commit;
        if sealed {
            transaction.sealed_for_commit = true;
            transaction.commit_thread = Some(std::thread::current().id());
        }
        CommitSeal {
            session: self,
            generation: transaction.generation,
            sealed,
        }
    }

    /// `SET LOCAL`: applies a setting until the transaction ends. Outside a
    /// transaction block it has no effect, as in PostgreSQL.
    pub(crate) fn set_local_setting(&self, name: &str, value: &str) -> Result<(), CassieError> {
        if super::is_projection_write_mode(name) {
            return self.set_projection_write_mode(Some(value));
        }
        let mut transaction = self.transaction.lock();
        if transaction.status != SessionTransactionStatus::InTransaction {
            return Ok(());
        }
        let name = name.to_ascii_lowercase();
        if !transaction.local_settings.contains(&name) {
            transaction.local_settings.push(name.clone());
        }
        drop(transaction);
        self.settings.lock().set(&name, value).map(|_| ())
    }

    pub(crate) fn rollback_transaction(&self) {
        let mut transaction = self.transaction.lock();
        let unique_owner_id = transaction.unique_owner_id;
        let session_state = transaction.session_state.take();
        transaction.status = SessionTransactionStatus::Idle;
        transaction.implicit = false;
        transaction.isolation = None;
        transaction.modes = TransactionModes::default();
        transaction.has_query = false;
        transaction.unique_owner_id = uuid::Uuid::new_v4();
        transaction.writes = SharedTransactionWrites::default();
        transaction.conflict_intents.clear();
        transaction.savepoints.clear();
        transaction.repeatable_read = None;
        transaction.catalog_overlay = None;
        transaction.catalog_base_version = None;
        transaction.catalog_views = super::catalog_overlay::TransactionCatalogViews::default();
        transaction.storage_overlay =
            Arc::new(Mutex::new(crate::midge::adapter::StorageOverlay::default()));
        transaction.local_settings.clear();
        transaction.projection_write = Arc::default();
        transaction.sealed_for_commit = false;
        transaction.commit_thread = None;
        // See `commit_transaction`: cleanup completes before the mutex is released.
        self.retire_schema_context();
        if let Some(waits) = self.transaction_unique_waits() {
            waits.release_owner(unique_owner_id);
        }
        if let Some(session_state) = session_state {
            self.restore_session_state(session_state);
        }
        self.end_transaction_objects(false);
        #[cfg(test)]
        retirement_tests::observe_retirement(self);
        drop(transaction);
    }
}

/// Releases a commit seal when COMMIT ends without retiring the transaction.
pub(crate) struct CommitSeal<'a> {
    session: &'a CassieSession,
    generation: u64,
    sealed: bool,
}

impl CommitSeal<'_> {
    /// Retires the transaction this COMMIT published. If another path already
    /// ended that transaction, or a later one has started, nothing is touched:
    /// COMMIT never resets a transaction it did not commit (#1290).
    pub(crate) fn retire_committed(&mut self) {
        if !self.sealed {
            // An enclosing COMMIT holds the seal; keep the previous behaviour
            // for this nested or re-entrant commit.
            self.session.commit_transaction();
            return;
        }
        self.sealed = false;
        let transaction = self.session.transaction.lock();
        if transaction.generation == self.generation && transaction.sealed_for_commit {
            self.session.retire_committed_locked(transaction);
        }
    }
}

impl Drop for CommitSeal<'_> {
    fn drop(&mut self) {
        if !self.sealed {
            return;
        }
        let mut transaction = self.session.transaction.lock();
        // A successful COMMIT already reset the state, and a later transaction
        // has a new generation; only the still-open sealed one is unsealed.
        if transaction.generation == self.generation {
            transaction.sealed_for_commit = false;
            transaction.commit_thread = None;
        }
    }
}

#[cfg(test)]
#[path = "retirement_tests.rs"]
mod retirement_tests;

#[cfg(test)]
#[path = "commit_seal_tests.rs"]
mod commit_seal_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn begin_with_read_only(session: &CassieSession, read_only: bool) {
        session
            .begin_transaction_with_modes(
                None,
                TransactionModes {
                    read_only: Some(read_only),
                    deferrable: None,
                },
            )
            .expect("begin transaction");
    }

    fn observed_replacement(
        session: &CassieSession,
        retire_old: bool,
    ) -> (Result<(), CassieError>, Option<(bool, bool)>) {
        let concurrent = session.clone();
        let (sent, received) = std::sync::mpsc::sync_channel(1);
        let mut worker = None;
        let mut transition = None;
        let result = session.ensure_transaction_writable_observed(|| {
            worker = Some(std::thread::spawn(move || {
                let old_failed = concurrent.is_transaction_failed();
                if retire_old {
                    concurrent.rollback_transaction();
                }
                let started = concurrent
                    .begin_transaction_with_modes(
                        None,
                        TransactionModes {
                            read_only: Some(false),
                            deferrable: None,
                        },
                    )
                    .is_ok();
                sent.send((old_failed, started))
                    .expect("replacement observed");
            }));
            transition = received
                .recv_timeout(std::time::Duration::from_secs(5))
                .ok();
        });
        worker.expect("replacement worker").join().expect("worker");
        (result, transition)
    }

    fn is_read_only_error(result: &Result<(), CassieError>) -> bool {
        matches!(result, Err(CassieError::Transaction {
            kind: TransactionErrorKind::ReadOnlyTransaction,
            message,
        }) if message == "cannot execute a write in a read-only transaction")
    }

    #[test]
    fn should_preserve_replacement_after_read_only_write_check() {
        // Arrange
        let session = CassieSession::new("root".to_owned(), None);
        begin_with_read_only(&session, true);

        // Act
        let (result, transition) = observed_replacement(&session, true);

        // Assert
        assert!(is_read_only_error(&result));
        assert_eq!(transition, Some((true, true)));
        assert!(session.is_transaction_active());
        assert_eq!(
            session.transaction_setting("transaction_read_only"),
            Some("off")
        );
    }

    #[test]
    fn should_preserve_new_transaction_after_idle_default_check() {
        // Arrange
        let session = CassieSession::new("root".to_owned(), None);
        session
            .set_setting("default_transaction_read_only", "on")
            .expect("read-only default");

        // Act
        let (result, transition) = observed_replacement(&session, false);

        // Assert
        assert!(is_read_only_error(&result));
        assert_eq!(transition, Some((false, true)));
        assert!(session.is_transaction_active());
        assert_eq!(
            session.transaction_setting("transaction_read_only"),
            Some("off")
        );
    }

    #[test]
    fn should_fail_current_read_only_transaction_on_write_check() {
        // Arrange
        let session = CassieSession::new("root".to_owned(), None);
        begin_with_read_only(&session, true);

        // Act
        let result = session.ensure_transaction_writable();

        // Assert
        assert!(is_read_only_error(&result));
        assert!(session.is_transaction_failed());
    }

    #[test]
    fn should_allow_current_read_write_transaction_on_write_check() {
        // Arrange
        let session = CassieSession::new("root".to_owned(), None);
        begin_with_read_only(&session, false);

        // Act
        let result = session.ensure_transaction_writable();

        // Assert
        assert!(result.is_ok());
        assert!(session.is_transaction_active());
    }

    #[test]
    fn should_preserve_failed_read_only_write_check_result() {
        // Arrange
        let session = CassieSession::new("root".to_owned(), None);
        begin_with_read_only(&session, true);
        session.mark_transaction_failed();

        // Act
        let result = session.ensure_transaction_writable();

        // Assert
        assert!(is_read_only_error(&result));
        assert!(session.is_transaction_failed());
    }

    #[test]
    fn should_preserve_failed_read_write_check_result() {
        // Arrange
        let session = CassieSession::new("root".to_owned(), None);
        begin_with_read_only(&session, false);
        session.mark_transaction_failed();

        // Act
        let result = session.ensure_transaction_writable();

        // Assert
        assert!(result.is_ok());
        assert!(session.is_transaction_failed());
    }

    #[test]
    fn should_fail_implicit_read_only_transaction_on_write_check() {
        // Arrange
        let session = CassieSession::new("root".to_owned(), None);
        session
            .set_setting("default_transaction_read_only", "on")
            .expect("read-only default");
        session
            .begin_implicit_transaction()
            .expect("implicit begin");

        // Act
        let result = session.ensure_transaction_writable();

        // Assert
        assert!(is_read_only_error(&result));
        assert!(session.is_transaction_failed());
    }

    #[test]
    fn should_apply_idle_default_to_write_check() {
        // Arrange
        let session = CassieSession::new("root".to_owned(), None);
        let writable = session.ensure_transaction_writable();
        session
            .set_setting("default_transaction_read_only", "on")
            .expect("read-only default");

        // Act
        let rejected = session.ensure_transaction_writable();

        // Assert
        assert!(writable.is_ok());
        assert!(is_read_only_error(&rejected));
        assert_eq!(session.transaction_status(), "idle");
    }

    #[test]
    fn should_preserve_read_only_mode_given_statement_fork_when_staged_write_is_attempted() {
        // Arrange
        let session = CassieSession::new("root".to_string(), None);
        session
            .begin_transaction_with_modes(
                None,
                TransactionModes {
                    read_only: Some(true),
                    deferrable: Some(true),
                },
            )
            .expect("read-only transaction");
        let batch = session.fork_statement_batch().expect("statement fork");

        // Act
        let rejected = batch
            .session()
            .stage_document_write(
                "ordinary_rows",
                "id".to_string(),
                serde_json::json!({"value": 1}),
            )
            .expect_err("fork must retain mode");

        // Assert
        assert_eq!(rejected.descriptor().sql_state, "25006");
        assert!(batch.session().transaction_writes().is_empty());
        assert!(session.transaction_writes().is_empty());
        assert_eq!(
            session.transaction_setting("transaction_read_only"),
            Some("on")
        );
    }

    #[test]
    fn should_restore_transaction_mode_given_savepoint_when_local_mode_change_is_rolled_back() {
        // Arrange
        let session = CassieSession::new("root".to_string(), None);
        session
            .begin_transaction(None)
            .expect("writable transaction");
        session.create_savepoint("before_mode").expect("savepoint");
        session
            .set_setting("transaction_read_only", "on")
            .expect("become read-only");

        // Act
        session
            .rollback_to_savepoint("before_mode")
            .expect("restore savepoint");
        session
            .stage_document_write(
                "ordinary_rows",
                "id".to_string(),
                serde_json::json!({"value": 1}),
            )
            .expect("restored mode permits staging");

        // Assert
        assert_eq!(
            session.transaction_setting("transaction_read_only"),
            Some("off")
        );
        assert_eq!(session.transaction_writes()["ordinary_rows"].len(), 1);
    }
}
