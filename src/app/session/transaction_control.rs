//! Session transaction characteristics and ownership boundaries.

use super::{
    Arc, CassieError, CassieSession, Mutex, SessionTransactionStatus, SharedTransactionWrites,
    TransactionIsolation,
};
use crate::app::TransactionErrorKind;
use crate::sql::ast::TransactionModes;

impl CassieSession {
    pub(crate) fn begin_transaction(
        &self,
        isolation: Option<TransactionIsolation>,
    ) -> Result<(), CassieError> {
        self.begin_transaction_with_modes(isolation, TransactionModes::default())
    }

    pub(crate) fn begin_transaction_with_modes(
        &self,
        isolation: Option<TransactionIsolation>,
        modes: TransactionModes,
    ) -> Result<(), CassieError> {
        if matches!(
            isolation,
            Some(TransactionIsolation::RepeatableRead | TransactionIsolation::Serializable)
        ) {
            return Err(CassieError::Unsupported(
                "only READ COMMITTED transaction isolation is supported".to_string(),
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
        let session_state = self.session_state_snapshot();
        let mut transaction = self.transaction.lock();
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
            transaction.implicit = false;
            transaction.isolation = isolation.or(transaction.isolation);
            transaction.modes.read_only = requested_modes.read_only.or(transaction.modes.read_only);
            transaction.modes.deferrable =
                requested_modes.deferrable.or(transaction.modes.deferrable);
            return Ok(());
        }
        transaction.status = SessionTransactionStatus::InTransaction;
        transaction.implicit = false;
        transaction.isolation = isolation;
        transaction.modes = modes;
        transaction.has_query = false;
        transaction.generation = transaction.generation.wrapping_add(1);
        transaction.writes = SharedTransactionWrites::default();
        transaction.conflict_intents.clear();
        transaction.savepoints.clear();
        transaction.catalog_overlay = None;
        transaction.catalog_base_version = None;
        transaction.storage_overlay =
            Arc::new(Mutex::new(crate::midge::adapter::StorageOverlay::default()));
        transaction.session_state = Some(session_state);
        transaction.local_settings.clear();
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
        if self.is_transaction_read_only() {
            self.mark_transaction_failed();
            return Err(CassieError::Transaction {
                kind: TransactionErrorKind::ReadOnlyTransaction,
                message: "cannot execute a write in a read-only transaction".to_string(),
            });
        }
        Ok(())
    }

    pub(crate) fn is_transaction_read_only(&self) -> bool {
        let transaction = self.transaction.lock();
        if transaction.status == SessionTransactionStatus::Idle {
            drop(transaction);
            self.default_transaction_read_only()
        } else {
            transaction.modes.read_only.unwrap_or(false)
        }
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
        self.discard_sequence_restarts();
        let mut transaction = self.transaction.lock();
        transaction.status = SessionTransactionStatus::Idle;
        transaction.implicit = false;
        transaction.isolation = None;
        transaction.modes = TransactionModes::default();
        transaction.has_query = false;
        transaction.writes = SharedTransactionWrites::default();
        transaction.conflict_intents.clear();
        transaction.savepoints.clear();
        transaction.catalog_overlay = None;
        transaction.catalog_base_version = None;
        transaction.storage_overlay =
            Arc::new(Mutex::new(crate::midge::adapter::StorageOverlay::default()));
        let session_state = transaction.session_state.take();
        let locals = std::mem::take(&mut transaction.local_settings);
        drop(transaction);
        self.end_transaction_objects(true);
        if let Some(state) = session_state {
            let mut settings = self.settings.lock();
            for name in locals {
                settings.restore_from(&name, &state.settings);
            }
        }
    }

    /// `SET LOCAL`: applies a setting until the transaction ends. Outside a
    /// transaction block it has no effect, as in PostgreSQL.
    pub(crate) fn set_local_setting(&self, name: &str, value: &str) -> Result<(), CassieError> {
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
        self.restore_sequence_restarts();
        let mut transaction = self.transaction.lock();
        let session_state = transaction.session_state.take();
        transaction.status = SessionTransactionStatus::Idle;
        transaction.implicit = false;
        transaction.isolation = None;
        transaction.modes = TransactionModes::default();
        transaction.has_query = false;
        transaction.writes = SharedTransactionWrites::default();
        transaction.conflict_intents.clear();
        transaction.savepoints.clear();
        transaction.catalog_overlay = None;
        transaction.catalog_base_version = None;
        transaction.storage_overlay =
            Arc::new(Mutex::new(crate::midge::adapter::StorageOverlay::default()));
        transaction.local_settings.clear();
        drop(transaction);
        if let Some(session_state) = session_state {
            self.restore_session_state(session_state);
        }
        self.end_transaction_objects(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
