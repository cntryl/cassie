//! Savepoint payloads exclude the transaction-wide retained read view.

use super::{
    BTreeSet, CassieError, CassieSession, Catalog, SessionSavepoint, SessionTransactionStatus,
    MAX_SAVEPOINTS_PER_TRANSACTION,
};

impl CassieSession {
    pub(crate) fn create_savepoint(&self, name: &str) -> Result<(), CassieError> {
        let mut transaction = self.transaction.lock();
        match transaction.status {
            SessionTransactionStatus::InTransaction => {}
            SessionTransactionStatus::Failed => return Err(CassieError::failed_transaction()),
            SessionTransactionStatus::Idle => {
                return Err(CassieError::savepoint_outside_transaction("SAVEPOINT"));
            }
        }
        if transaction.savepoints.len() >= MAX_SAVEPOINTS_PER_TRANSACTION {
            return Err(CassieError::ResourceLimit(
                "savepoint limit exceeded".to_string(),
            ));
        }

        let writes = transaction.writes.clone();
        let conflict_intents = transaction.conflict_intents.clone();
        let modes = transaction.modes;
        let catalog_overlay = transaction
            .catalog_overlay
            .as_ref()
            .map(Catalog::detached_transaction_snapshot);
        let catalog_base_version = transaction.catalog_base_version;
        let storage_overlay = transaction.storage_overlay.lock().clone();
        let unique_keys = self
            .transaction_unique_waits()
            .map_or_else(BTreeSet::new, |waits| {
                waits.snapshot_owner_keys(transaction.unique_owner_id)
            });
        let catalog_views = transaction.catalog_views.clone();
        transaction.savepoints.push(SessionSavepoint {
            name: name.to_ascii_lowercase(),
            object_mark: self.session_objects_mark(),
            modes,
            writes,
            conflict_intents,
            catalog_overlay,
            catalog_base_version,
            catalog_views,
            storage_overlay,
            session_state: self.session_state_snapshot(),
            unique_keys,
        });
        Ok(())
    }

    pub(crate) fn rollback_to_savepoint(&self, name: &str) -> Result<(), CassieError> {
        let mut transaction = self.transaction.lock();
        if !matches!(
            transaction.status,
            SessionTransactionStatus::InTransaction | SessionTransactionStatus::Failed
        ) {
            return Err(CassieError::savepoint_outside_transaction(
                "ROLLBACK TO SAVEPOINT",
            ));
        }

        let normalized = name.to_ascii_lowercase();
        let Some(index) = transaction
            .savepoints
            .iter()
            .rposition(|savepoint| savepoint.name == normalized)
        else {
            return Err(CassieError::missing_savepoint(&normalized));
        };

        // Compute and retire only identities absent from the restored savepoint.
        // The shared owner keeps advances for identities that still exist there.
        if transaction.savepoints[index]
            .storage_overlay
            .has_sequence_cursors()
        {
            let controls = crate::midge::adapter::PublicationControlScope::current_query_controls()
                .ok_or_else(|| {
                    CassieError::Unsupported(
                        "savepoint sequence recovery requires query controls".into(),
                    )
                })?;
            transaction.savepoints[index]
                .storage_overlay
                .prune_sequence_cursors_with(
                    transaction.savepoints[..=index]
                        .iter()
                        .map(|savepoint| &savepoint.storage_overlay),
                    &controls,
                )?;
        }
        transaction.modes = transaction.savepoints[index].modes;
        transaction.writes = transaction.savepoints[index].writes.clone();
        let conflict_intents = transaction.savepoints[index].conflict_intents.clone();
        let catalog_overlay = transaction.savepoints[index]
            .catalog_overlay
            .as_ref()
            .map(Catalog::detached_transaction_snapshot);
        let catalog_base_version = transaction.savepoints[index].catalog_base_version;
        let storage_overlay = transaction.savepoints[index].storage_overlay.clone();
        let session_state = transaction.savepoints[index].session_state.clone();
        let object_mark = transaction.savepoints[index].object_mark;
        let unique_owner_id = transaction.unique_owner_id;
        let unique_keys = transaction.savepoints[index].unique_keys.clone();
        transaction.conflict_intents = conflict_intents;
        transaction.catalog_overlay = catalog_overlay;
        transaction.catalog_base_version = catalog_base_version;
        transaction.catalog_views = transaction.savepoints[index].catalog_views.clone();
        *transaction.storage_overlay.lock() = storage_overlay;
        transaction.savepoints.truncate(index + 1);
        transaction.status = SessionTransactionStatus::InTransaction;
        drop(transaction);
        self.retire_schema_context();
        if let Some(waits) = self.transaction_unique_waits() {
            waits.restore_owner_keys(unique_owner_id, &unique_keys)?;
        }
        self.restore_session_state(session_state);
        self.rollback_session_objects_to(object_mark);
        Ok(())
    }

    pub(crate) fn release_savepoint(&self, name: &str) -> Result<(), CassieError> {
        let mut transaction = self.transaction.lock();
        match transaction.status {
            SessionTransactionStatus::InTransaction => {}
            SessionTransactionStatus::Failed => return Err(CassieError::failed_transaction()),
            SessionTransactionStatus::Idle => {
                return Err(CassieError::savepoint_outside_transaction(
                    "RELEASE SAVEPOINT",
                ));
            }
        }

        let normalized = name.to_ascii_lowercase();
        let Some(index) = transaction
            .savepoints
            .iter()
            .rposition(|savepoint| savepoint.name == normalized)
        else {
            return Err(CassieError::missing_savepoint(&normalized));
        };

        transaction.savepoints.truncate(index);
        Ok(())
    }
}
