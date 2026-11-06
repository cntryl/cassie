//! Isolated statement mutation forks and atomic publication into the session.

use super::{
    Arc, CassieError, CassieSession, Catalog, Mutex, SessionTransactionState,
    SessionTransactionStatus, SharedTransactionWrites, StatementMutationBatch,
    StatementMutationDelta, StatementRowMutation, StorageOverlay,
};

impl CassieSession {
    pub(crate) fn fork_statement_batch(&self) -> Result<StatementMutationBatch, CassieError> {
        let transaction = self.transaction.lock();
        if transaction.status == SessionTransactionStatus::Failed {
            return Err(CassieError::failed_transaction());
        }

        let base_transaction_active = transaction.status == SessionTransactionStatus::InTransaction;
        let working_state = self.statement_fork_state(&transaction)?;
        drop(transaction);
        let base_generation = working_state.generation;
        let base_writes = working_state.writes.clone();
        let conflict_intents = &working_state.conflict_intents;
        let catalog_overlay = working_state
            .catalog_overlay
            .as_ref()
            .map(Catalog::detached_transaction_snapshot);
        let catalog_base_version = working_state.catalog_base_version;
        let storage_overlay = working_state.storage_overlay.lock().clone();
        let base_conflict_intent_count = conflict_intents.len();
        let session = Self {
            statement_text: self.statement_text.clone(),
            user: self.user.clone(),
            database: self.database.clone(),
            access: self.access,
            schema_context: Arc::clone(&self.schema_context),
            search_path: Arc::new(Mutex::new(self.search_path())),
            settings: Arc::new(Mutex::new(self.settings.lock().clone())),
            backend_pid: Arc::clone(&self.backend_pid),
            sequence_values: Arc::clone(&self.sequence_values),
            transaction: Arc::new(Mutex::new(working_state)),
            transaction_unique_waits: Arc::clone(&self.transaction_unique_waits),
            owns_transaction_lifecycle: false,
            procedure_calls: Arc::new(Mutex::new(self.procedure_calls.lock().clone())),
            random: Arc::clone(&self.random),
            session_objects: Arc::clone(&self.session_objects),
            transaction_clock: Arc::clone(&self.transaction_clock),
        };

        Ok(StatementMutationBatch {
            session,
            base_transaction_active,
            base_generation,
            base_writes,
            base_conflict_intent_count,
            base_catalog_overlay: catalog_overlay,
            base_catalog_base_version: catalog_base_version,
            base_storage_overlay: storage_overlay,
        })
    }

    fn statement_fork_state(
        &self,
        current: &SessionTransactionState,
    ) -> Result<SessionTransactionState, CassieError> {
        let active = current.status == SessionTransactionStatus::InTransaction;
        let mut storage_overlay = if active {
            current.storage_overlay.lock().clone()
        } else {
            StorageOverlay::default()
        };
        let controls = crate::midge::adapter::PublicationControlScope::current_query_controls();
        storage_overlay.attach_sequence_fork_lease(controls.as_ref())?;
        Ok(SessionTransactionState {
            status: SessionTransactionStatus::InTransaction,
            implicit: false,
            isolation: if active { current.isolation } else { None },
            modes: if active {
                current.modes
            } else {
                crate::sql::ast::TransactionModes {
                    read_only: Some(self.default_transaction_read_only()),
                    deferrable: Some(self.default_transaction_deferrable()),
                }
            },
            has_query: active && current.has_query,
            generation: current.generation,
            repeatable_read: if active {
                current.repeatable_read.clone()
            } else {
                None
            },
            catalog_views: if active {
                current.catalog_views.clone()
            } else {
                super::catalog_overlay::TransactionCatalogViews::default()
            },
            unique_owner_id: current.unique_owner_id,
            writes: if active {
                current.writes.clone()
            } else {
                SharedTransactionWrites::default()
            },
            conflict_intents: if active {
                current.conflict_intents.clone()
            } else {
                Vec::new()
            },
            savepoints: Vec::new(),
            catalog_overlay: if active {
                current
                    .catalog_overlay
                    .as_ref()
                    .map(Catalog::detached_transaction_snapshot)
            } else {
                None
            },
            catalog_base_version: if active {
                current.catalog_base_version
            } else {
                None
            },
            storage_overlay: Arc::new(Mutex::new(storage_overlay)),
            session_state: None,
            local_settings: Vec::new(),
        })
    }

    pub(crate) fn publish_statement_batch(
        &self,
        batch: &StatementMutationBatch,
    ) -> Result<(), CassieError> {
        let delta = batch.delta()?;
        let mut transaction = self.transaction.lock();
        if transaction.status != SessionTransactionStatus::InTransaction {
            return Err(CassieError::Execution(
                "statement mutation requires an active transaction".to_string(),
            ));
        }

        if transaction.generation != batch.base_generation {
            return Err(CassieError::Execution(
                "session transaction changed while statement was executing".to_owned(),
            ));
        }
        for mutation in &delta.rows {
            let current = transaction
                .writes
                .get(&mutation.collection)
                .and_then(|writes| writes.get(&mutation.id));
            if current != mutation.before.as_ref() {
                return Err(CassieError::Execution(
                    "session transaction changed while statement was executing".to_string(),
                ));
            }
        }

        let current_catalog_version = transaction.catalog_overlay.as_ref().map(Catalog::version);
        let base_catalog_version = batch.base_catalog_overlay.as_ref().map(Catalog::version);
        if current_catalog_version != base_catalog_version
            || transaction.catalog_base_version != batch.base_catalog_base_version
            || *transaction.storage_overlay.lock() != batch.base_storage_overlay
        {
            return Err(CassieError::Execution(
                "session transaction changed while statement was executing".to_string(),
            ));
        }

        // Complete nested fork ownership transfer before publishing any rows
        // or catalog changes into this parent statement.
        let batch_transaction = batch.session.transaction.lock();
        let mut published = batch_transaction.storage_overlay.lock().clone();
        let controls = crate::midge::adapter::PublicationControlScope::current_query_controls();
        {
            let parent = transaction.storage_overlay.lock();
            published.inherit_sequence_fork_lease(&parent, controls.as_ref())?;
        }

        for mutation in delta.rows {
            if let Some(change) = mutation.after {
                let writes = Arc::make_mut(&mut transaction.writes);
                Arc::make_mut(writes.entry(mutation.collection).or_default())
                    .insert(mutation.id, change);
            } else {
                let writes = Arc::make_mut(&mut transaction.writes);
                let remove_collection =
                    writes.get_mut(&mutation.collection).is_some_and(|writes| {
                        let writes = Arc::make_mut(writes);
                        writes.remove(&mutation.id);
                        writes.is_empty()
                    });
                if remove_collection {
                    writes.remove(&mutation.collection);
                }
            }
        }
        transaction.conflict_intents.extend(delta.conflict_intents);
        transaction.catalog_overlay = batch_transaction
            .catalog_overlay
            .as_ref()
            .map(Catalog::detached_transaction_snapshot);
        transaction.catalog_base_version = batch_transaction.catalog_base_version;
        transaction.catalog_views = batch_transaction.catalog_views.clone();
        *transaction.storage_overlay.lock() = published;
        Ok(())
    }
}

impl StatementMutationBatch {
    pub(crate) fn session(&self) -> &CassieSession {
        &self.session
    }

    pub(crate) const fn has_enclosing_transaction(&self) -> bool {
        self.base_transaction_active
    }

    pub(in crate::app) fn into_session(self) -> CassieSession {
        self.session
    }

    fn delta(&self) -> Result<StatementMutationDelta, CassieError> {
        let transaction = self.session.transaction.lock();
        if transaction.status != SessionTransactionStatus::InTransaction {
            return Err(CassieError::Execution(
                "statement mutation batch is not active".to_string(),
            ));
        }
        if transaction.conflict_intents.len() < self.base_conflict_intent_count {
            return Err(CassieError::Execution(
                "statement mutation changed prior conflict intents".to_string(),
            ));
        }

        let mut rows = Vec::new();
        for (collection, working_rows) in transaction.writes.iter() {
            let base_rows = self.base_writes.get(collection);
            for (id, change) in working_rows.iter() {
                let before = base_rows.and_then(|base| base.get(id));
                if before != Some(change) {
                    rows.push(StatementRowMutation {
                        collection: collection.clone(),
                        id: id.clone(),
                        before: before.cloned(),
                        after: Some(change.clone()),
                    });
                }
            }
        }
        for (collection, base_rows) in self.base_writes.iter() {
            let working_rows = transaction.writes.get(collection);
            for (id, change) in base_rows.iter() {
                if working_rows.is_none_or(|working| !working.contains_key(id)) {
                    rows.push(StatementRowMutation {
                        collection: collection.clone(),
                        id: id.clone(),
                        before: Some(change.clone()),
                        after: None,
                    });
                }
            }
        }

        Ok(StatementMutationDelta {
            rows,
            conflict_intents: transaction.conflict_intents[self.base_conflict_intent_count..]
                .to_vec(),
        })
    }
}
