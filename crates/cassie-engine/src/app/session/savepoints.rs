//! Savepoint payloads exclude the transaction-wide retained read view.

#[cfg(test)]
mod authority_test_hooks;

use super::{
    BTreeSet, CassieError, CassieSession, Catalog, SessionSavepoint, SessionTransactionStatus,
    MAX_SAVEPOINTS_PER_TRANSACTION,
};

impl CassieSession {
    #[cfg(any(test, feature = "transport-test-hooks"))]
    #[doc(hidden)]
    pub fn rollback_epoch_for_test(&self) -> u64 {
        self.transaction.lock().rollback_epoch
    }

    #[doc(hidden)]

    pub fn create_savepoint(&self, name: &str) -> Result<(), CassieError> {
        let mut transaction = self.transaction.lock();
        self.ensure_operation_current_locked(&transaction)?;
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
        let projection_write_mode = transaction.projection_write.mode();
        let catalog_overlay = transaction
            .catalog_overlay
            .as_ref()
            .map(Catalog::detached_transaction_snapshot);
        let catalog_base_version = transaction.catalog_base_version;
        let storage_overlay = transaction.storage_overlay.lock().clone();
        let private_outputs = transaction.private_outputs.clone();
        let unique_keys = self
            .transaction_unique_waits()
            .map_or_else(BTreeSet::new, |waits| {
                waits.snapshot_owner_keys(transaction.unique_owner_id)
            });
        let row_keys = self
            .transaction_unique_waits()
            .map_or_else(BTreeSet::new, |waits| {
                waits.snapshot_owner_rows(transaction.unique_owner_id)
            });
        let catalog_views = transaction.catalog_views.clone();
        let session_writes = transaction.session_writes.clone();
        transaction.savepoints.push(SessionSavepoint {
            name: name.to_ascii_lowercase(),
            object_mark: self.session_objects_mark(),
            modes,
            projection_write_mode,
            writes,
            conflict_intents,
            catalog_overlay,
            catalog_base_version,
            catalog_views,
            storage_overlay,
            private_outputs,
            session_state: self.session_state_snapshot(),
            session_writes,
            unique_keys,
            row_keys,
        });
        Ok(())
    }

    #[cfg(any(test, feature = "transport-test-hooks"))]
    #[doc(hidden)]
    pub fn rollback_to_savepoint(&self, name: &str) -> Result<(), CassieError> {
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
        self.ensure_given_operation_current_locked(&transaction, authority)?;
        if !matches!(
            transaction.status,
            SessionTransactionStatus::InTransaction | SessionTransactionStatus::Failed
        ) {
            return Err(CassieError::savepoint_outside_transaction(
                "ROLLBACK TO SAVEPOINT",
            ));
        }

        let index = savepoint_index(&transaction, name)?;

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
                        &controls.storage_request(),
                    )
                    .map_err(CassieError::from)
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
        restore_savepoint_modes(&mut transaction, index);
        transaction.writes = transaction.savepoints[index].writes.clone();
        let conflict_intents = transaction.savepoints[index].conflict_intents.clone();
        let catalog_overlay = transaction.savepoints[index]
            .catalog_overlay
            .as_ref()
            .map(Catalog::detached_transaction_snapshot);
        let catalog_base_version = transaction.savepoints[index].catalog_base_version;
        let storage_overlay = transaction.savepoints[index].storage_overlay.clone();
        let private_outputs = transaction.savepoints[index].private_outputs.clone();
        let session_state = transaction.savepoints[index].session_state.clone();
        transaction.session_writes = transaction.savepoints[index].session_writes.clone();
        let object_mark = transaction.savepoints[index].object_mark;
        let unique_owner_id = transaction.unique_owner_id;
        #[cfg(test)]
        let restored_epoch = transaction.rollback_epoch;
        let unique_keys = transaction.savepoints[index].unique_keys.clone();
        let row_keys = transaction.savepoints[index].row_keys.clone();
        transaction.conflict_intents = conflict_intents;
        transaction.catalog_overlay = catalog_overlay;
        transaction.catalog_base_version = catalog_base_version;
        transaction.catalog_views = transaction.savepoints[index].catalog_views.clone();
        *transaction.storage_overlay.lock() = storage_overlay;
        transaction.private_outputs = private_outputs;
        transaction.savepoints.truncate(index + 1);
        transaction.status = SessionTransactionStatus::InTransaction;
        self.retire_schema_context();
        if let Some(waits) = self.transaction_unique_waits() {
            let restored = waits.restore_owner_keys(unique_owner_id, &unique_keys);
            if let Err(error) = restored {
                drop(transaction);
                #[cfg(test)]
                authority_test_hooks::after_restore_error(
                    self,
                    unique_owner_id,
                    restored_epoch,
                    &error,
                );
                return Err(error);
            }
            waits.restore_owner_rows(unique_owner_id, &row_keys)?;
        }
        self.restore_session_state(session_state);
        self.rollback_session_objects_to(object_mark);
        #[cfg(test)]
        self.observe_savepoint_cleanup_for_test();
        drop(transaction);
        Ok(())
    }

    pub(crate) fn release_savepoint(&self, name: &str) -> Result<(), CassieError> {
        let mut transaction = self.transaction.lock();
        self.ensure_operation_current_locked(&transaction)?;
        match transaction.status {
            SessionTransactionStatus::InTransaction => {}
            SessionTransactionStatus::Failed => return Err(CassieError::failed_transaction()),
            SessionTransactionStatus::Idle => {
                return Err(CassieError::savepoint_outside_transaction(
                    "RELEASE SAVEPOINT",
                ));
            }
        }

        let index = savepoint_index(&transaction, name)?;

        transaction.savepoints.truncate(index);
        Ok(())
    }
}

fn savepoint_index(
    transaction: &super::SessionTransactionState,
    name: &str,
) -> Result<usize, CassieError> {
    let normalized = name.to_ascii_lowercase();
    transaction
        .savepoints
        .iter()
        .rposition(|savepoint| savepoint.name == normalized)
        .ok_or_else(|| CassieError::missing_savepoint(&normalized))
}

/// Restores the transaction modes and projection write declaration recorded by
/// a savepoint. Sticky projection effect history is deliberately left alone.
fn restore_savepoint_modes(transaction: &mut super::SessionTransactionState, index: usize) {
    let savepoint = &transaction.savepoints[index];
    let (modes, projection_write_mode) = (savepoint.modes, savepoint.projection_write_mode);
    transaction.modes = modes;
    transaction
        .projection_write
        .restore_mode(projection_write_mode);
}
