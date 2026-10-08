//! Whole-session COW overlay ownership for a single statement read scope.

use super::{
    staged_snapshot_accounting_overflow, CassieError, CassieSession, CollectionChanges, Mutex,
    SessionTransactionState, SessionTransactionStatus, SharedTransactionWrites,
    StagedWriteSnapshot, TransactionRowChange, TransactionWrites,
};
use crate::midge::adapter::{Midge, StatementDataRead};
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

#[derive(Debug)]
pub(crate) struct StatementOverlay {
    writes: SharedTransactionWrites,
    active_transaction: bool,
    identity: Weak<Mutex<SessionTransactionState>>,
    _memory: QueryMemoryReservation,
}

impl StatementOverlay {
    pub(crate) fn matches_session(&self, session: &CassieSession) -> bool {
        std::ptr::eq(self.identity.as_ptr(), Arc::as_ptr(&session.transaction))
    }
}

impl CassieSession {
    pub(crate) fn capture_statement_read(
        &self,
        midge: &Arc<Midge>,
        database: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Arc<StatementDataRead>, CassieError> {
        if controls.is_cancelled() {
            return Err(CassieError::QueryCancelled);
        }
        if controls.is_timed_out() {
            return Err(CassieError::DeadlineExceeded);
        }
        let transaction = self.transaction.lock();
        let node = 11 * (size_of::<String>() + size_of::<Arc<CollectionChanges>>())
            + 16 * size_of::<usize>();
        // The weak identity keeps only the original Arc allocation, not its
        // savepoint/conflict payloads, alive if the session is later dropped.
        let base = size_of::<StatementOverlay>()
            .checked_add(2 * size_of::<usize>())
            .and_then(|bytes| bytes.checked_add(size_of::<Mutex<SessionTransactionState>>()))
            .and_then(|bytes| bytes.checked_add(2 * size_of::<usize>()))
            .and_then(|bytes| bytes.checked_add(size_of::<TransactionWrites>()))
            .and_then(|bytes| bytes.checked_add(2 * size_of::<usize>()))
            .and_then(|bytes| bytes.checked_add(transaction.writes.len().max(1).checked_mul(node)?))
            .ok_or_else(staged_snapshot_accounting_overflow)?;
        let bytes = transaction
            .writes
            .iter()
            .try_fold(base, |bytes, (name, changes)| {
                let changes_bytes = collection_snapshot_bytes(changes)?;
                bytes
                    .checked_add(name.capacity())
                    .and_then(|bytes| bytes.checked_add(changes_bytes))
                    .ok_or_else(staged_snapshot_accounting_overflow)
            })?;
        let memory = controls.reserve_query_memory(bytes)?;
        let overlay = Arc::new(StatementOverlay {
            writes: Arc::clone(&transaction.writes),
            active_transaction: transaction.status != SessionTransactionStatus::Idle,
            identity: Arc::downgrade(&self.transaction),
            _memory: memory,
        });
        // Keep the session state lock through capture; staged-map changes
        // cannot interleave between the borrowed estimate and the Data owner.
        StatementDataRead::capture_with_overlay(midge, database, controls, Some(overlay))
    }

    pub(crate) fn read_transaction_is_active(&self) -> bool {
        CURRENT_OVERLAY.with(|current| {
            current
                .borrow()
                .as_ref()
                .filter(|owner| {
                    std::ptr::eq(owner.identity.as_ptr(), Arc::as_ptr(&self.transaction))
                })
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
                .filter(|owner| {
                    std::ptr::eq(owner.identity.as_ptr(), Arc::as_ptr(&self.transaction))
                })
                .map(|owner| {
                    owner.writes.iter().any(|(name, changes)| {
                        !changes.is_empty() && crate::catalog::name_matches(name, collection)
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
            .filter(|owner| {
                std::ptr::eq(owner.identity.as_ptr(), Arc::as_ptr(&session.transaction))
            })
            .map(|owner| StagedWriteSnapshot::matching(&owner.writes, collection))
    })
}

pub(crate) struct SessionReadScope {
    previous: Option<Arc<StatementOverlay>>,
    _thread: PhantomData<Rc<()>>,
}

impl SessionReadScope {
    pub(crate) fn enter(owner: Option<&Arc<StatementOverlay>>) -> Self {
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
