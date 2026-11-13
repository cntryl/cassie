use super::{
    Cassie, CassieError, CassieSession, QueryResult, QueryStatement, TransactionAction,
    TransactionErrorKind, TransactionStatement,
};

#[path = "query_transactions/repeatable_write.rs"]
mod repeatable_write;

impl CassieError {
    /// `PostgreSQL` 25P02 for work attempted inside a failed transaction block.
    pub(crate) fn failed_transaction() -> Self {
        Self::Transaction {
            kind: TransactionErrorKind::FailedTransaction,
            message:
                "current transaction is aborted, commands ignored until end of transaction block"
                    .to_owned(),
        }
    }

    /// `PostgreSQL` 25P01 for a savepoint control outside a transaction block.
    pub(crate) fn savepoint_outside_transaction(command: &str) -> Self {
        Self::Transaction {
            kind: TransactionErrorKind::NoActiveTransaction,
            message: format!("{command} can only be used in transaction blocks"),
        }
    }

    /// `PostgreSQL` 3B001 for a savepoint name absent from the current block.
    pub(crate) fn missing_savepoint(name: &str) -> Self {
        Self::Transaction {
            kind: TransactionErrorKind::InvalidSavepoint,
            message: format!("savepoint \"{name}\" does not exist"),
        }
    }
}

impl Cassie {
    pub(crate) fn ensure_transaction_recovery(
        session: &CassieSession,
        parsed: &crate::sql::ast::ParsedStatement,
    ) -> Result<(), CassieError> {
        let recovery = matches!(
            &parsed.statement,
            QueryStatement::Transaction(TransactionStatement {
                action: TransactionAction::Commit
                    | TransactionAction::Rollback
                    | TransactionAction::RollbackTo { .. },
                ..
            })
        );
        if session.is_transaction_failed() && !recovery {
            return Err(CassieError::failed_transaction());
        }
        Ok(())
    }

    pub(super) fn execute_transaction_statement(
        &self,
        session: &CassieSession,
        statement: &TransactionStatement,
        controls: &crate::runtime::QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        let savepoint_control = match statement.action {
            TransactionAction::Savepoint { .. } => Some("SAVEPOINT"),
            TransactionAction::RollbackTo { .. } => Some("ROLLBACK TO SAVEPOINT"),
            TransactionAction::Release { .. } => Some("RELEASE SAVEPOINT"),
            _ => None,
        };
        if let Some(control) = savepoint_control.filter(|_| session.is_implicit_transaction()) {
            session.mark_transaction_failed();
            return Err(CassieError::savepoint_outside_transaction(control));
        }
        // PostgreSQL ends a failed block on COMMIT by rolling it back.
        if statement.chain
            && (!session.is_transaction_active() && !session.is_transaction_failed()
                || session.is_implicit_transaction())
        {
            return Err(CassieError::Transaction {
                kind: TransactionErrorKind::NoActiveTransaction,
                message: "AND CHAIN can only be used in transaction blocks".to_string(),
            });
        }
        let characteristics = statement.chain.then(|| {
            if session.is_transaction_failed() {
                (None, crate::sql::ast::TransactionModes::default())
            } else {
                session.transaction_characteristics()
            }
        });
        let failed_commit = matches!(statement.action, TransactionAction::Commit)
            && session.is_transaction_failed();
        let command = match &statement.action {
            TransactionAction::Begin => "BEGIN",
            TransactionAction::Commit if failed_commit => "ROLLBACK",
            TransactionAction::Commit => "COMMIT",
            TransactionAction::Rollback | TransactionAction::RollbackTo { .. } => "ROLLBACK",
            TransactionAction::Savepoint { .. } => "SAVEPOINT",
            TransactionAction::Release { .. } => "RELEASE",
        };
        let output: Result<QueryResult, CassieError> = (|| {
            let mut result = QueryResult::try_builder(controls)?;
            result.try_set_command(command)?;
            result.finish()
        })();
        if output.is_err()
            && matches!(statement.action, TransactionAction::Commit)
            && session.is_repeatable_read_write()
        {
            session.rollback_transaction();
        }
        let output = output?;

        match &statement.action {
            TransactionAction::Begin => {
                session.begin_transaction_with_modes(statement.isolation, statement.modes)?;
            }
            TransactionAction::Commit if failed_commit => session.rollback_transaction(),
            TransactionAction::Commit => {
                self.commit_transaction_with_controls(session, controls)?;
            }
            TransactionAction::Rollback => session.rollback_transaction(),
            TransactionAction::Savepoint { name } => session.create_savepoint(name)?,
            TransactionAction::RollbackTo { name } => session.rollback_to_savepoint(name)?,
            TransactionAction::Release { name } => session.release_savepoint(name)?,
        }
        if let Some((isolation, modes)) = characteristics {
            session.begin_transaction_with_modes(isolation, modes)?;
        }
        Ok(output)
    }

    pub(crate) fn commit_transaction(&self, session: &CassieSession) -> Result<(), CassieError> {
        let controls = self.runtime.query_controls(std::time::Instant::now());
        self.commit_transaction_with_controls(session, &controls)
    }

    pub(crate) fn commit_transaction_with_controls(
        &self,
        session: &CassieSession,
        controls: &crate::runtime::QueryExecutionControls,
    ) -> Result<(), CassieError> {
        self.commit_transaction_using_catalog_guard(session, None, controls)
    }

    pub(super) fn commit_transaction_holding_catalog<'a>(
        &'a self,
        session: &CassieSession,
        publication: parking_lot::RecursiveRwLockWriteGuard<'a, ()>,
        controls: &crate::runtime::QueryExecutionControls,
    ) -> Result<(), CassieError> {
        self.commit_transaction_using_catalog_guard(session, Some(publication), controls)
    }

    fn commit_transaction_using_catalog_guard<'a>(
        &'a self,
        session: &CassieSession,
        held_publication: Option<parking_lot::RecursiveRwLockWriteGuard<'a, ()>>,
        controls: &crate::runtime::QueryExecutionControls,
    ) -> Result<(), CassieError> {
        let result = self.commit_transaction_inner(session, held_publication, controls);
        // PostgreSQL ends the block when COMMIT itself fails: nothing is
        // published and the session returns to idle without a ROLLBACK.
        if result.is_err() && (session.is_transaction_active() || session.is_transaction_failed())
        {
            session.rollback_transaction();
        }
        result
    }

    /// Rejects COMMIT when storage needs recovery or no healthy transaction is active.
    fn ensure_commit_admissible(&self, session: &CassieSession) -> Result<(), CassieError> {
        if let Err(error) = self.check_storage_recovery_required() {
            session.rollback_transaction();
            return Err(error);
        }
        if session.is_transaction_failed() {
            return Err(CassieError::failed_transaction());
        }
        if !session.is_transaction_active() {
            return Err(CassieError::Execution(
                "COMMIT requires an active transaction".to_string(),
            ));
        }
        Ok(())
    }

    /// Fails the transaction when another commit published catalog changes first.
    fn ensure_commit_catalog_current(&self, session: &CassieSession) -> Result<(), CassieError> {
        if session.transaction_catalog_base_version() != Some(self.publication_catalog.version()) {
            session.mark_transaction_failed();
            return Err(CassieError::Transaction {
                kind: TransactionErrorKind::SerializationFailure,
                message: "catalog changed while a transactional DDL block was active".to_owned(),
            });
        }
        Ok(())
    }

    fn commit_transaction_inner<'a>(
        &'a self,
        session: &CassieSession,
        held_publication: Option<parking_lot::RecursiveRwLockWriteGuard<'a, ()>>,
        controls: &crate::runtime::QueryExecutionControls,
    ) -> Result<(), CassieError> {
        let _entry_read_scope = crate::midge::adapter::StatementReadScope::enter(None);
        let _entry_overlay_scope = super::SessionReadScope::enter(None);
        let _publication_controls =
            crate::midge::adapter::PublicationControlScope::enter(Some(controls));
        self.ensure_commit_admissible(session)?;
        let (storage_overlay, _sequence_buffers, _sequence_directory) = session
            .transaction_storage_overlay_snapshot()
            .with_sequence_cursors(controls)?;
        let catalog_overlay = session.transaction_catalog_overlay().filter(|catalog| {
            // A detached but untouched catalog is read authority, not a write.
            // Publishing it would overwrite unrelated newer metadata and make
            // repeated IF NOT EXISTS commands advance the global schema epoch.
            session.transaction_catalog_base_version() != Some(catalog.version())
                || !storage_overlay.changes.is_empty()
        });
        let removed_role_grant_targets = catalog_overlay
            .as_ref()
            .map_or_default(|catalog| self.role_grant_targets_removed_by_transaction(catalog));
        if let Some(catalog) = &catalog_overlay {
            storage_overlay.apply_sequence_cursors_to_catalog(catalog, controls)?;
            let staged = storage_overlay.staged_sequence_object_ids(controls)?;
            catalog.refresh_published_sequence_cursors(&self.publication_catalog, &staged);
        }
        let database = session
            .current_database()
            .unwrap_or(&self.default_database)
            .to_owned();
        let repeatable_write = session.is_repeatable_read_write();
        let catalog_publication = self.acquire_commit_catalog_guard(
            catalog_overlay.is_some(),
            repeatable_write,
            held_publication,
            controls,
        )?;
        if catalog_overlay.is_some() {
            self.ensure_commit_catalog_current(session)?;
        }
        let transactional_ddl = catalog_overlay
            .as_ref()
            .map(|_| (database.as_str(), &storage_overlay));
        #[cfg(test)]
        output_admission_tests::cancel_before_index_guard_if_armed();
        let index_guard =
            crate::midge::adapter::IndexCleanupCommitGuard::capture(&storage_overlay, controls)
                .inspect_err(|_| session.mark_transaction_failed())?;
        let (row_collections, referential) = self.transaction_commit_write_gates(session);
        let gate_names = if index_guard.is_empty() {
            None
        } else {
            Some(
                index_guard
                    .gate_names(&row_collections, controls)
                    .inspect_err(|_| session.mark_transaction_failed())?,
            )
        };
        let collections = gate_names
            .as_ref()
            .map_or(row_collections.as_slice(), |names| names.values.as_slice());
        let publish = || {
            index_guard.validate(&self.midge, controls)?;
            self.publish_transaction_rows(
                session,
                &database,
                catalog_overlay.as_ref(),
                collections,
                controls,
                transactional_ddl,
            )
        };
        let committed = if repeatable_write {
            self.midge
                .with_transaction_write_gates(collections, referential, controls, || {
                    if referential {
                        self.validate_staged_foreign_keys(session)?;
                        self.validate_staged_parent_references(session)?;
                    }
                    publish()
                })
        } else if referential {
            self.midge.with_collection_write_gates(collections, || {
                self.validate_staged_foreign_keys(session)?;
                self.validate_staged_parent_references(session)?;
                publish()
            })
        } else {
            self.midge.with_collection_gates(collections, publish)
        }
        .inspect_err(|_| session.mark_transaction_failed())?;

        if let Some(snapshot) = catalog_overlay.as_ref() {
            self.publication_catalog
                .publish_transaction_snapshot(snapshot);
        }
        self.cleanup_committed_role_grants(removed_role_grant_targets);
        session.commit_transaction();
        if let Some(publication) = catalog_publication {
            publication.retire();
        }
        self.finish_staged_write_batches(committed, Some(controls));
        Ok(())
    }

    fn cleanup_committed_role_grants(
        &self,
        removed_role_grant_targets: Vec<(String, Vec<crate::catalog::RoleGrantTarget>)>,
    ) {
        for (database_family, targets) in removed_role_grant_targets {
            if let Err(error) = self
                .midge
                .delete_role_object_grants_for_targets(&database_family, &targets)
            {
                self.runtime.record_storage_access("schema", true, false);
                tracing::warn!(
                    database_family,
                    target_count = targets.len(),
                    %error,
                    "committed DDL left inert role grants for startup reconciliation"
                );
            } else {
                self.runtime.record_storage_access("schema", true, true);
            }
        }
    }

    fn bump_transaction_schema_epoch(&self) -> Result<(), CassieError> {
        let schema_epoch = self
            .midge
            .bump_schema_epoch()
            .map_err(|error| CassieError::Storage(format!("bump schema epoch: {error}")))?;
        self.runtime.record_storage_access("schema", true, true);
        self.runtime.set_schema_epoch(schema_epoch);
        self.runtime.invalidate_plan_cache();
        self.runtime.invalidate_execution_result_cache();
        Ok(())
    }
}

#[cfg(test)]
#[path = "query_transactions/output_admission_tests.rs"]
mod output_admission_tests;

#[cfg(test)]
#[path = "query_transactions/repeatable_write_tests.rs"]
mod repeatable_write_tests;
