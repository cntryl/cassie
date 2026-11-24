//! Whole-session COW overlay ownership for a single statement read scope.

use super::{
    staged_snapshot_accounting_overflow, CassieError, CassieSession, CollectionChanges, Mutex,
    SessionTransactionState, SessionTransactionStatus, SharedTransactionWrites,
    StagedWriteSnapshot, TransactionRowChange, TransactionWrites,
};
use crate::executor::retained_memory::add;
use crate::midge::adapter::{
    Midge, NativeStatementReadScope, NativeStatementSnapshot, StorageRequestContext,
};
use crate::runtime::accounted::json;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use std::cell::RefCell;
use std::marker::PhantomData;
use std::mem::size_of;
use std::rc::Rc;
use std::sync::{Arc, Weak};

thread_local! {
    static CURRENT_OVERLAY: RefCell<Option<Arc<StatementOverlay>>> = const { RefCell::new(None) };
}

/// Engine-owned command view over a storage-owned native snapshot.
pub(crate) struct StatementDataRead {
    snapshot: Arc<NativeStatementSnapshot>,
    overlay: Option<Arc<StatementOverlay>>,
    private_outputs: Option<MaterializedReads>,
    memory: QueryMemoryReservation,
}

#[derive(Clone)]
enum MaterializedReads {
    All(Arc<crate::app::PrivateOutputs>),
    One(Arc<PrivateOutput>),
}

impl MaterializedReads {
    fn get(&self, logical: &str) -> Option<&Arc<PrivateOutput>> {
        match self {
            Self::All(outputs) => outputs.get(logical),
            Self::One(output) => Some(output),
        }
    }

    fn accounted_bytes(&self) -> Result<usize, CassieError> {
        match self {
            Self::All(outputs) => outputs.values().try_fold(0, |sum, output| {
                cassie_core::request::accounted::retained::add(sum, output.accounted_bytes()?)
                    .map_err(|error| CassieError::ResourceLimit(error.to_string()))
            }),
            Self::One(output) => output.accounted_bytes(),
        }
    }
}

impl std::fmt::Debug for StatementDataRead {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StatementDataRead")
            .field("data_epoch", &self.snapshot.data_epoch())
            .finish_non_exhaustive()
    }
}

impl StatementDataRead {
    #[cfg(test)]
    pub(crate) fn shares_native_snapshot_for_test(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.snapshot, &other.snapshot)
    }

    #[cfg(test)]
    pub(crate) fn projection_metadata_for_test(
        &self,
        projection: &str,
    ) -> Result<Option<cassie_sql::catalog_declarations::ProjectionMeta>, CassieError> {
        self.snapshot.projection_metadata_for_test(projection)
    }

    pub(crate) fn from_native_snapshot(
        snapshot: Arc<NativeStatementSnapshot>,
        controls: &QueryExecutionControls,
    ) -> Result<Arc<Self>, CassieError> {
        let memory = controls.reserve_query_memory(size_of::<Self>() + 2 * size_of::<usize>())?;
        Ok(Arc::new(Self {
            snapshot,
            overlay: None,
            private_outputs: None,
            memory,
        }))
    }

    pub(crate) fn capture(
        midge: &Arc<Midge>,
        database: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Arc<Self>, CassieError> {
        Self::capture_with_overlay(midge, database, controls, None)
    }

    pub(crate) fn capture_with_overlay(
        midge: &Arc<Midge>,
        database: &str,
        controls: &QueryExecutionControls,
        overlay: Option<Arc<StatementOverlay>>,
    ) -> Result<Arc<Self>, CassieError> {
        Self::capture_with_authority(midge, database, controls, overlay, false)
    }

    pub(crate) fn capture_repeatable_read_with_overlay(
        midge: &Arc<Midge>,
        database: &str,
        controls: &QueryExecutionControls,
        overlay: Option<Arc<StatementOverlay>>,
    ) -> Result<Arc<Self>, CassieError> {
        Self::capture_with_authority(midge, database, controls, overlay, true)
    }

    fn capture_with_authority(
        midge: &Arc<Midge>,
        database: &str,
        controls: &QueryExecutionControls,
        overlay: Option<Arc<StatementOverlay>>,
        current_catalog: bool,
    ) -> Result<Arc<Self>, CassieError> {
        let context = controls.storage_request();
        let snapshot =
            NativeStatementSnapshot::capture(midge, database, &context, current_catalog)?;
        let memory = controls.reserve_query_memory(size_of::<Self>() + 2 * size_of::<usize>())?;
        Ok(Arc::new(Self {
            snapshot,
            private_outputs: None,
            overlay,
            memory,
        }))
    }

    pub(crate) fn with_overlay(
        retained: &Self,
        overlay: Option<Arc<StatementOverlay>>,
        controls: &QueryExecutionControls,
    ) -> Result<Arc<Self>, CassieError> {
        let context = StorageRequestContext::new(
            Some(&retained.snapshot),
            controls.interruption(),
            controls.request_memory(),
        )
        .with_scan_control(controls.query_scan_control_scope());
        context.check_interruption()?;
        let memory = controls.reserve_query_memory(size_of::<Self>() + 2 * size_of::<usize>())?;
        Ok(Arc::new(Self {
            snapshot: Arc::clone(&retained.snapshot),
            private_outputs: retained.private_outputs.clone(),
            overlay,
            memory,
        }))
    }

    pub(crate) fn matches(&self, midge: &Arc<Midge>, database: &str) -> bool {
        self.snapshot.matches(midge, database)
    }

    pub(crate) fn matches_current_family(
        &self,
        midge: &Arc<Midge>,
        database: &str,
    ) -> Result<bool, CassieError> {
        Ok(self.snapshot.matches_current_family(midge, database)?)
    }

    pub(crate) fn data_epoch(&self) -> u64 {
        self.snapshot.data_epoch()
    }

    pub(crate) fn overlay(&self) -> Option<&Arc<StatementOverlay>> {
        self.overlay.as_ref()
    }

    pub(crate) fn command_accounted_bytes(&self) -> Result<usize, CassieError> {
        let command = cassie_core::request::accounted::retained::add(
            self.memory.bytes(),
            self.overlay
                .as_ref()
                .map_or(0, |owner| owner.accounted_bytes()),
        )
        .map_err(|error| CassieError::ResourceLimit(error.to_string()))?;
        cassie_core::request::accounted::retained::add(
            command,
            self.private_outputs
                .as_ref()
                .map_or(Ok(0), MaterializedReads::accounted_bytes)?,
        )
        .map_err(|error| CassieError::ResourceLimit(error.to_string()))
    }

    pub(crate) fn private_owner_accounted_bytes(&self) -> Result<usize, CassieError> {
        cassie_core::request::accounted::retained::add(
            self.memory.bytes(),
            self.snapshot.accounted_bytes(),
        )
        .map_err(|error| CassieError::ResourceLimit(error.to_string()))
    }

    pub(crate) fn retain_bound_output(
        owner: &mut Arc<Self>,
        logical: &str,
        private_outputs: Option<&Arc<crate::app::PrivateOutputs>>,
    ) {
        if let Some(outputs) = private_outputs {
            Arc::get_mut(owner)
                .expect("new private command owner")
                .private_outputs = outputs.get(logical).cloned().map(MaterializedReads::One);
        }
    }

    pub(crate) fn with_materialized_outputs(
        retained: &Self,
        overlay: Arc<StatementOverlay>,
        private_outputs: Option<&Arc<crate::app::PrivateOutputs>>,
        controls: &QueryExecutionControls,
    ) -> Result<Arc<Self>, CassieError> {
        let mut owner = Self::with_overlay(retained, Some(overlay), controls)?;
        Arc::get_mut(&mut owner)
            .expect("new command owner")
            .private_outputs =
            private_outputs.map(|outputs| MaterializedReads::All(Arc::clone(outputs)));
        Ok(owner)
    }

    pub(crate) fn enter_materialized_output(
        controls: &QueryExecutionControls,
        logical: &str,
        output: &str,
    ) -> Option<StatementReadScope> {
        let private = controls
            .statement_read()?
            .private_outputs
            .as_ref()?
            .get(logical)?;
        (private.name == output).then(|| StatementReadScope::enter(Some(&private.read)))
    }
}

/// Engine session wrapper that also installs the matching storage snapshot scope.
pub(crate) struct StatementReadScope {
    _native: NativeStatementReadScope,
}

impl StatementReadScope {
    pub(crate) fn enter(owner: Option<&Arc<StatementDataRead>>) -> Self {
        let native = owner.map(|owner| {
            NativeStatementReadScope::enter(
                Some(&owner.snapshot),
                owner
                    .overlay()
                    .and_then(|overlay| overlay.checkpoint_storage().cloned()),
            )
        });
        Self {
            _native: native.unwrap_or_else(|| NativeStatementReadScope::enter(None, None)),
        }
    }
}

#[derive(Debug)]
pub(crate) struct StatementOverlay {
    writes: Option<SharedTransactionWrites>,
    checkpoint_storage: Option<Arc<crate::midge::adapter::CheckpointStorageSnapshot>>,
    active_transaction: bool,
    identity: Weak<Mutex<SessionTransactionState>>,
    generation: u64,
    memory: QueryMemoryReservation,
}

#[derive(Debug)]
pub(crate) struct PrivateOutput {
    pub(crate) name: String,
    pub(crate) read: Arc<StatementDataRead>,
    pub(crate) memory: QueryMemoryReservation,
}

pub(crate) type PrivateOutputs = std::collections::BTreeMap<String, Arc<PrivateOutput>>;

impl PrivateOutput {
    pub(crate) fn accounted_bytes(&self) -> Result<usize, CassieError> {
        cassie_core::request::accounted::retained::add(
            self.memory.bytes(),
            self.read.private_owner_accounted_bytes()?,
        )
        .map_err(|error| CassieError::ResourceLimit(error.to_string()))
    }
}

impl StatementOverlay {
    pub(crate) fn checkpoint_storage(
        &self,
    ) -> Option<&Arc<crate::midge::adapter::CheckpointStorageSnapshot>> {
        self.checkpoint_storage.as_ref()
    }

    pub(crate) const fn accounted_bytes(&self) -> usize {
        self.memory.bytes()
    }

    pub(crate) fn matches_session(&self, session: &CassieSession) -> bool {
        std::ptr::eq(self.identity.as_ptr(), Arc::as_ptr(&session.transaction))
            && self.generation == session.transaction_generation()
    }
}

impl CassieSession {
    pub(crate) fn retain_private_materialized_output(
        &self,
        midge: &Arc<Midge>,
        default_database: &str,
        logical: &str,
        output: &str,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        let mut transaction = self.transaction.lock();
        self.ensure_operation_current_locked(&transaction)?;
        let repeatable_read_write = transaction.isolation
            == Some(crate::sql::ast::TransactionIsolation::RepeatableRead)
            && transaction.repeatable_read.as_ref().map_or_else(
                || {
                    transaction.modes.read_only == Some(false)
                        || transaction
                            .savepoints
                            .iter()
                            .any(|saved| saved.modes.read_only == Some(false))
                },
                |owner| owner.has_history(),
            );
        if transaction.status != SessionTransactionStatus::InTransaction || !repeatable_read_write {
            return Ok(());
        }
        let storage = Arc::clone(&transaction.storage_overlay);
        let database = self.current_database().unwrap_or(default_database);
        let context = controls.storage_request();
        let storage_guard = storage.lock();
        let snapshot = midge.capture_private_materialized_snapshot(
            &storage_guard,
            database,
            output,
            &context,
        )?;
        let Some(snapshot) = snapshot else {
            return Ok(());
        };
        let read = StatementDataRead::from_native_snapshot(snapshot, controls)?;
        let memory = controls.reserve_query_memory(add(
            size_of::<PrivateOutput>(),
            add(output.len(), logical.len())?,
        )?)?;
        let next = PrivateOutput {
            name: output.to_owned(),
            read,
            memory,
        };
        Arc::make_mut(transaction.private_outputs.get_or_insert_with(Arc::default))
            .insert(logical.to_owned(), Arc::new(next));
        Ok(())
    }

    pub(crate) fn capture_statement_read(
        &self,
        midge: &Arc<Midge>,
        database: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Arc<StatementDataRead>, CassieError> {
        if controls.is_canceled() {
            return Err(CassieError::QueryCanceled);
        }
        if controls.is_timed_out() {
            return Err(CassieError::DeadlineExceeded);
        }
        if let Some(owner) = self.repeatable_statement_read(midge, database, controls)? {
            return Ok(owner);
        }
        let transaction = self.transaction.lock();
        let overlay = self.capture_statement_overlay_locked(&transaction, controls)?;
        // Keep the session state lock through capture; staged-map changes
        // cannot interleave between the borrowed estimate and the Data owner.
        if !transaction.implicit
            && transaction.isolation == Some(crate::sql::ast::TransactionIsolation::RepeatableRead)
        {
            StatementDataRead::capture_repeatable_read_with_overlay(
                midge,
                database,
                controls,
                Some(overlay),
            )
        } else {
            StatementDataRead::capture_with_overlay(midge, database, controls, Some(overlay))
        }
    }

    #[cfg(test)]
    pub(crate) fn capture_statement_overlay(
        &self,
        controls: &QueryExecutionControls,
    ) -> Result<Arc<StatementOverlay>, CassieError> {
        let transaction = self.transaction.lock();
        self.capture_statement_overlay_locked(&transaction, controls)
    }

    pub(super) fn capture_statement_overlay_locked(
        &self,
        transaction: &SessionTransactionState,
        controls: &QueryExecutionControls,
    ) -> Result<Arc<StatementOverlay>, CassieError> {
        // READ COMMITTED's scoped native transaction already includes staged
        // storage. A retained REPEATABLE READ base is committed-only and must
        // instead merge this command's own changes over that base.
        let checkpoint_storage = if !transaction.implicit
            && transaction.isolation == Some(crate::sql::ast::TransactionIsolation::RepeatableRead)
        {
            transaction
                .storage_overlay
                .lock()
                .copy_checkpoint_snapshot(&controls.storage_request())?
        } else {
            None
        };
        self.capture_overlay_from_writes(
            (!transaction.writes.is_empty()).then_some(&transaction.writes),
            transaction.status != SessionTransactionStatus::Idle,
            transaction.generation,
            checkpoint_storage,
            controls,
        )
    }

    pub(crate) fn rebind_statement_read(
        &self,
        owner: &StatementDataRead,
        controls: &QueryExecutionControls,
    ) -> Result<Arc<StatementDataRead>, CassieError> {
        let original = owner.overlay().ok_or_else(|| {
            CassieError::Execution("statement batch lost its command overlay".to_owned())
        })?;
        let overlay = self.capture_overlay_from_writes(
            original.writes.as_ref(),
            original.active_transaction,
            self.transaction_generation(),
            original.checkpoint_storage.clone(),
            controls,
        )?;
        StatementDataRead::with_overlay(owner, Some(overlay), controls)
    }

    fn capture_overlay_from_writes(
        &self,
        writes: Option<&SharedTransactionWrites>,
        active_transaction: bool,
        generation: u64,
        checkpoint_storage: Option<Arc<crate::midge::adapter::CheckpointStorageSnapshot>>,
        controls: &QueryExecutionControls,
    ) -> Result<Arc<StatementOverlay>, CassieError> {
        controls.check_interruption()?;
        let node = 11 * (size_of::<String>() + size_of::<Arc<CollectionChanges>>())
            + 16 * size_of::<usize>();
        // The weak identity keeps only the original Arc allocation, not its
        // savepoint/conflict payloads, alive if the session is later dropped.
        let base = size_of::<StatementOverlay>()
            .checked_add(2 * size_of::<usize>())
            .and_then(|bytes| bytes.checked_add(size_of::<Mutex<SessionTransactionState>>()))
            .and_then(|bytes| bytes.checked_add(2 * size_of::<usize>()))
            .ok_or_else(staged_snapshot_accounting_overflow)?;
        // Do not retain a possibly allocated empty BTree root. None is still
        // an authoritative captured empty view, including its transaction flag.
        let bytes = if let Some(writes) = writes {
            let base = base
                .checked_add(size_of::<TransactionWrites>())
                .and_then(|bytes| bytes.checked_add(2 * size_of::<usize>()))
                .and_then(|bytes| bytes.checked_add(writes.len().checked_mul(node)?))
                .ok_or_else(staged_snapshot_accounting_overflow)?;
            writes.iter().try_fold(base, |bytes, (name, changes)| {
                let changes_bytes = collection_snapshot_bytes(changes)?;
                bytes
                    .checked_add(name.capacity())
                    .and_then(|bytes| bytes.checked_add(changes_bytes))
                    .ok_or_else(staged_snapshot_accounting_overflow)
            })?
        } else {
            base
        };
        let memory = controls.reserve_query_memory(bytes)?;
        Ok(Arc::new(StatementOverlay {
            writes: writes.cloned(),
            checkpoint_storage,
            active_transaction,
            identity: Arc::downgrade(&self.transaction),
            generation,
            memory,
        }))
    }

    pub(crate) fn read_transaction_is_active(&self) -> bool {
        CURRENT_OVERLAY.with(|current| {
            current
                .borrow()
                .as_ref()
                .filter(|owner| owner.matches_session(self))
                .map_or_else(
                    || self.transaction_status() != "idle",
                    |owner| owner.active_transaction,
                )
        })
    }

    pub(super) fn captured_has_collection_changes(&self, collection: &str) -> Option<bool> {
        CURRENT_OVERLAY.with(|current| {
            current
                .borrow()
                .as_ref()
                .filter(|owner| owner.matches_session(self))
                .map(|owner| {
                    owner.writes.as_ref().is_some_and(|writes| {
                        writes.iter().any(|(name, changes)| {
                            !changes.is_empty() && crate::catalog::name_matches(name, collection)
                        })
                    })
                })
        })
    }

    pub(crate) fn read_document_change(
        &self,
        collection: &str,
        id: &str,
    ) -> Option<TransactionRowChange> {
        if let Some(snapshot) = captured_snapshot(self, collection) {
            return snapshot.ordered_changes().get(id).cloned();
        }
        self.document_change(collection, id)
    }
}

pub(super) fn captured_snapshot(
    session: &CassieSession,
    collection: &str,
) -> Option<StagedWriteSnapshot> {
    CURRENT_OVERLAY.with(|current| {
        current
            .borrow()
            .as_ref()
            .filter(|owner| owner.matches_session(session))
            .map(|owner| {
                owner
                    .writes
                    .as_deref()
                    .map_or_else(StagedWriteSnapshot::default, |writes| {
                        StagedWriteSnapshot::matching(writes, collection)
                    })
            })
    })
}

pub(crate) struct SessionReadScope {
    previous: Option<Arc<StatementOverlay>>,
    _thread: PhantomData<Rc<()>>,
}

impl SessionReadScope {
    #[doc(hidden)]
    pub fn enter(owner: Option<&Arc<StatementOverlay>>) -> Self {
        Self {
            previous: CURRENT_OVERLAY.with(|current| current.replace(owner.cloned())),
            _thread: PhantomData,
        }
    }
}

impl Drop for SessionReadScope {
    fn drop(&mut self) {
        CURRENT_OVERLAY.with(|current| current.replace(self.previous.take()));
    }
}

pub(super) fn collection_snapshot_bytes(changes: &CollectionChanges) -> Result<usize, CassieError> {
    // Charge a full internal BTree node per entry, including unused key/value slots.
    // An empty owned map can retain its root, so its estimate includes one node too.
    let node_bytes = 11
        * (std::mem::size_of::<String>() + std::mem::size_of::<TransactionRowChange>())
        + 16 * std::mem::size_of::<usize>();
    let container_bytes =
        std::mem::size_of::<CollectionChanges>() + 2 * std::mem::size_of::<usize>();
    let map_bytes = changes
        .len()
        .max(1)
        .checked_mul(node_bytes)
        .and_then(|bytes| bytes.checked_add(container_bytes))
        .ok_or_else(staged_snapshot_accounting_overflow)?;
    changes.iter().try_fold(map_bytes, |bytes, (id, change)| {
        let payload_bytes = match change {
            TransactionRowChange::Upsert(payload) => json::retained_bytes(payload)?,
            TransactionRowChange::Delete => 0,
        };
        bytes
            .checked_add(id.capacity())
            .and_then(|bytes| bytes.checked_add(payload_bytes))
            .ok_or_else(staged_snapshot_accounting_overflow)
    })
}
