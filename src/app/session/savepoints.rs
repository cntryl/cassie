//! Savepoint payloads exclude the transaction-wide retained read view.

#[cfg(test)]
mod authority_test_hooks;

use super::{
    BTreeSet, CassieError, CassieSession, Catalog, SessionSavepoint, SessionTransactionStatus,
    MAX_SAVEPOINTS_PER_TRANSACTION,
};

impl CassieSession {
    #[cfg(test)]
    pub(crate) fn rollback_epoch_for_test(&self) -> u64 {
        self.transaction.lock().rollback_epoch
    }

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
        self.rollback_to_savepoint_inner(name, None, || {})
    }

    pub(crate) fn rollback_to_savepoint_for_operation(
        &self,
        name: &str,
        authority: Option<&super::FailureAuthority>,
    ) -> Result<(), CassieError> {
        self.rollback_to_savepoint_inner(name, authority, || {})
    }

    #[cfg(test)]
    pub(super) fn rollback_to_savepoint_observed(
        &self,
        name: &str,
        after_prune: impl FnOnce(),
    ) -> Result<(), CassieError> {
        self.rollback_to_savepoint_inner(name, None, after_prune)
    }

    fn rollback_to_savepoint_inner(
        &self,
        name: &str,
        authority: Option<&super::FailureAuthority>,
        after_prune: impl FnOnce(),
    ) -> Result<(), CassieError> {
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

        // Invalidate prior forks before the first possible rollback mutation.
        // An ordinary prune error occurs before its retain; restore the old
        // epoch while still holding the parent mutex. An unwind keeps it advanced.
        let previous_epoch = transaction.rollback_epoch;
        transaction.rollback_epoch = previous_epoch.checked_add(1).ok_or_else(|| {
            CassieError::ResourceLimit("savepoint rollback epoch exhausted".to_owned())
        })?;
        let adopted = authority.is_some_and(|authority| {
            authority.advance_savepoint(self, &transaction, previous_epoch)
        });
        if transaction.savepoints[index]
            .storage_overlay
            .has_sequence_cursors()
        {
            #[cfg(test)]
            authority_test_hooks::before_prune(self, transaction.rollback_epoch);
            let pruning = (|| {
                let controls =
                    crate::midge::adapter::PublicationControlScope::current_query_controls()
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
                    )
            })();
            if let Err(error) = pruning {
                transaction.rollback_epoch = previous_epoch;
                if adopted {
                    if let Some(authority) = authority {
                        authority.revert_savepoint(self, &transaction, previous_epoch);
                    }
                }
                return Err(error);
            }
            after_prune();
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
        #[cfg(test)]
        let restored_epoch = transaction.rollback_epoch;
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
            let restored = waits.restore_owner_keys(unique_owner_id, &unique_keys);
            #[cfg(test)]
            if let Err(error) = &restored {
                authority_test_hooks::after_restore_error(
                    self,
                    unique_owner_id,
                    restored_epoch,
                    error,
                );
            }
            restored?;
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
