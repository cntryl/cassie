//! Private same-database statement Data ownership. Write transactions remain fresh.

use super::{CassieError, Midge, TransactionMode};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use std::cell::RefCell;
use std::mem::size_of;
use std::sync::Arc;

thread_local! {
    static CURRENT_READ: RefCell<Option<Arc<StatementDataRead>>> = const { RefCell::new(None) };
}

pub(crate) struct StatementDataRead {
    // Release the snapshot before its engine owner, then its admitted metadata.
    tx: cntryl_midge::Transaction,
    midge: Arc<Midge>,
    database: String,
    data_epoch: u64,
    _memory: QueryMemoryReservation,
    overlay: Option<Arc<crate::app::StatementOverlay>>,
}

impl std::fmt::Debug for StatementDataRead {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StatementDataRead")
            .field("database", &self.database)
            .finish_non_exhaustive()
    }
}

impl StatementDataRead {
    pub(crate) fn capture(
        midge: &Arc<Midge>,
        database: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Arc<Self>, CassieError> {
        Self::capture_with_overlay(midge, database, controls, None)
    }

    pub(crate) const fn data_epoch(&self) -> u64 {
        self.data_epoch
    }

    pub(crate) fn overlay(&self) -> Option<&Arc<crate::app::StatementOverlay>> {
        self.overlay.as_ref()
    }

    pub(crate) fn capture_with_overlay(
        midge: &Arc<Midge>,
        database: &str,
        controls: &QueryExecutionControls,
        overlay: Option<Arc<crate::app::StatementOverlay>>,
    ) -> Result<Arc<Self>, CassieError> {
        if controls.is_cancelled() {
            return Err(CassieError::QueryCancelled);
        }
        if controls.is_timed_out() {
            return Err(CassieError::DeadlineExceeded);
        }
        // Midge owns transaction-internal snapshot retention. Cassie owns this
        // Arc allocation, its inline transaction and copied database identity.
        let bytes = size_of::<Self>()
            .checked_add(2 * size_of::<usize>())
            .and_then(|bytes| bytes.checked_add(database.len()))
            .ok_or_else(|| {
                CassieError::ResourceLimit("statement read owner size overflow".to_string())
            })?;
        let memory = controls.reserve_query_memory(bytes)?;
        let tx = midge.database_tx(database, TransactionMode::ReadOnly)?;
        let epoch_scratch =
            controls.reserve_query_memory(super::key_encoding::data_epoch_key_scratch_bytes())?;
        let data_epoch = Midge::load_data_epoch_from_tx(&tx)?;
        drop(epoch_scratch);
        Ok(Arc::new(Self {
            tx,
            midge: Arc::clone(midge),
            database: database.to_owned(),
            data_epoch,
            _memory: memory,
            overlay,
        }))
    }
}

/// Standalone reads retain their existing inline transaction. Shared reads
/// retain the entire admitted owner, including its engine and snapshot charge.
/// Private constructors enforce exactly one populated owner without adding a
/// heap allocation to previously standalone readonly calls.
pub(super) struct DataReadTransaction {
    fresh: Option<cntryl_midge::Transaction>,
    shared: Option<Arc<StatementDataRead>>,
}

impl DataReadTransaction {
    pub(super) fn fresh(tx: cntryl_midge::Transaction) -> Self {
        Self {
            fresh: Some(tx),
            shared: None,
        }
    }

    fn shared(owner: Arc<StatementDataRead>) -> Self {
        Self {
            fresh: None,
            shared: Some(owner),
        }
    }
}

impl std::ops::Deref for DataReadTransaction {
    type Target = cntryl_midge::Transaction;

    fn deref(&self) -> &Self::Target {
        if let Some(owner) = &self.shared {
            &owner.tx
        } else {
            self.fresh
                .as_ref()
                .expect("private constructor provides a readonly owner")
        }
    }
}

pub(crate) struct StatementReadScope {
    previous: Option<Arc<StatementDataRead>>,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl StatementReadScope {
    pub(crate) fn enter(owner: Option<&Arc<StatementDataRead>>) -> Self {
        let previous = CURRENT_READ.with(|current| current.replace(owner.cloned()));
        Self {
            previous,
            _thread: std::marker::PhantomData,
        }
    }
}

impl Drop for StatementReadScope {
    fn drop(&mut self) {
        CURRENT_READ.with(|current| current.replace(self.previous.take()));
    }
}

impl Midge {
    pub(super) fn statement_data_read(&self, database: &str) -> Option<DataReadTransaction> {
        CURRENT_READ.with(|current| {
            current
                .borrow()
                .as_ref()
                .filter(|owner| {
                    std::ptr::eq(self, Arc::as_ptr(&owner.midge))
                        && owner.database.eq_ignore_ascii_case(database)
                })
                .map(|owner| DataReadTransaction::shared(Arc::clone(owner)))
        })
    }
}
