//! Private same-database statement Data ownership. Write transactions remain fresh.

use super::{CassieError, Midge, TransactionMode};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use std::cell::RefCell;
use std::mem::size_of;
use std::sync::Arc;

thread_local! {
    static CURRENT_READ: RefCell<Option<Arc<StatementDataRead>>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DescriptorAuthority {
    StatementSnapshot,
    CurrentCatalog,
}

struct StatementDataSnapshot {
    // Release the snapshot before its engine owner, then its admitted metadata.
    tx: crate::midge::adapter::AdapterTransaction,
    midge: Arc<Midge>,
    database: String,
    family_id: cntryl_midge::ColumnFamilyId,
    data_epoch: u64,
    descriptor_authority: DescriptorAuthority,
    _memory: QueryMemoryReservation,
}

pub(crate) struct StatementDataRead {
    snapshot: Arc<StatementDataSnapshot>,
    overlay: Option<Arc<crate::app::StatementOverlay>>,
    memory: QueryMemoryReservation,
}

impl std::fmt::Debug for StatementDataRead {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StatementDataRead")
            .field("database", &self.snapshot.database)
            .finish_non_exhaustive()
    }
}

impl StatementDataRead {
    /// Additional per-command retained backing; the immutable transaction core
    /// is shared with the transaction owner and is admitted there once.
    pub(crate) fn command_accounted_bytes(&self) -> Result<usize, CassieError> {
        crate::executor::retained_memory::add(
            self.memory.bytes(),
            self.overlay
                .as_ref()
                .map_or(0, |owner| owner.accounted_bytes()),
        )
    }

    pub(crate) fn capture(
        midge: &Arc<Midge>,
        database: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Arc<Self>, CassieError> {
        Self::capture_with_overlay(midge, database, controls, None)
    }

    pub(crate) fn matches(&self, midge: &Arc<Midge>, database: &str) -> bool {
        Arc::ptr_eq(&self.snapshot.midge, midge)
            && self.snapshot.database.eq_ignore_ascii_case(database)
    }

    pub(crate) fn matches_current_family(
        &self,
        midge: &Arc<Midge>,
        database: &str,
    ) -> Result<bool, CassieError> {
        Ok(self.matches(midge, database)
            && midge.database_family(database)?.id() == self.snapshot.family_id)
    }

    pub(crate) fn data_epoch(&self) -> u64 {
        self.snapshot.data_epoch
    }

    pub(crate) fn overlay(&self) -> Option<&Arc<crate::app::StatementOverlay>> {
        self.overlay.as_ref()
    }

    pub(crate) fn with_overlay(
        retained: &Self,
        overlay: Option<Arc<crate::app::StatementOverlay>>,
        controls: &QueryExecutionControls,
    ) -> Result<Arc<Self>, CassieError> {
        controls.check_interruption()?;
        let memory = controls.reserve_query_memory(size_of::<Self>() + 2 * size_of::<usize>())?;
        Ok(Arc::new(Self {
            snapshot: Arc::clone(&retained.snapshot),
            overlay,
            memory,
        }))
    }

    pub(crate) fn capture_with_overlay(
        midge: &Arc<Midge>,
        database: &str,
        controls: &QueryExecutionControls,
        overlay: Option<Arc<crate::app::StatementOverlay>>,
    ) -> Result<Arc<Self>, CassieError> {
        Self::capture_with_authority(
            midge,
            database,
            controls,
            overlay,
            DescriptorAuthority::StatementSnapshot,
        )
    }

    pub(crate) fn capture_repeatable_read_with_overlay(
        midge: &Arc<Midge>,
        database: &str,
        controls: &QueryExecutionControls,
        overlay: Option<Arc<crate::app::StatementOverlay>>,
    ) -> Result<Arc<Self>, CassieError> {
        Self::capture_with_authority(
            midge,
            database,
            controls,
            overlay,
            DescriptorAuthority::CurrentCatalog,
        )
    }

    fn capture_with_authority(
        midge: &Arc<Midge>,
        database: &str,
        controls: &QueryExecutionControls,
        overlay: Option<Arc<crate::app::StatementOverlay>>,
        descriptor_authority: DescriptorAuthority,
    ) -> Result<Arc<Self>, CassieError> {
        if controls.is_canceled() {
            return Err(CassieError::QueryCanceled);
        }
        if controls.is_timed_out() {
            return Err(CassieError::DeadlineExceeded);
        }
        // Midge owns transaction-internal snapshot retention. Cassie owns this
        // Arc allocation, its inline transaction and copied database identity.
        let bytes = size_of::<StatementDataSnapshot>()
            .checked_add(2 * size_of::<usize>())
            .and_then(|bytes| bytes.checked_add(database.len()))
            .ok_or_else(|| {
                CassieError::ResourceLimit("statement read owner size overflow".to_string())
            })?;
        let memory = controls.reserve_query_memory(bytes)?;
        let command_memory =
            controls.reserve_query_memory(size_of::<Self>() + 2 * size_of::<usize>())?;
        let family = midge.database_family(database)?;
        let family_id = family.id();
        let tx = midge.begin_adapter_tx(family_id, TransactionMode::ReadOnly)?;
        // The temporary family handle does not become statement-owned backing.
        drop(family);
        let epoch_scratch =
            controls.reserve_query_memory(super::key_encoding::data_epoch_key_scratch_bytes())?;
        let data_epoch = Midge::load_data_epoch_from_tx(&tx)?;
        drop(epoch_scratch);
        let snapshot = Arc::new(StatementDataSnapshot {
            tx,
            midge: Arc::clone(midge),
            database: database.to_owned(),
            family_id,
            data_epoch,
            descriptor_authority,
            _memory: memory,
        });
        Ok(Arc::new(Self {
            snapshot,
            overlay,
            memory: command_memory,
        }))
    }
}

/// Standalone reads retain their existing inline transaction. Shared reads
/// retain the entire admitted owner, including its engine and snapshot charge.
/// Private constructors enforce exactly one populated owner without adding a
/// heap allocation to previously standalone readonly calls.
pub(super) struct DataReadTransaction {
    fresh: Option<crate::midge::adapter::AdapterTransaction>,
    shared: Option<Arc<StatementDataRead>>,
}

impl DataReadTransaction {
    pub(super) fn fresh(tx: crate::midge::adapter::AdapterTransaction) -> Self {
        Self {
            fresh: Some(tx),
            shared: None,
        }
    }

    pub(super) fn uses_current_descriptors(&self) -> bool {
        self.shared.as_ref().is_some_and(|owner| {
            owner.snapshot.descriptor_authority == DescriptorAuthority::CurrentCatalog
        })
    }

    fn shared(owner: Arc<StatementDataRead>) -> Self {
        Self {
            fresh: None,
            shared: Some(owner),
        }
    }
}

impl std::ops::Deref for DataReadTransaction {
    type Target = crate::midge::adapter::AdapterTransaction;

    fn deref(&self) -> &Self::Target {
        if let Some(owner) = &self.shared {
            &owner.snapshot.tx
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
    pub(super) fn statement_data_read_for_family(
        &self,
        family_id: cntryl_midge::ColumnFamilyId,
    ) -> Option<DataReadTransaction> {
        CURRENT_READ.with(|current| {
            current
                .borrow()
                .as_ref()
                .filter(|owner| {
                    std::ptr::eq(self, Arc::as_ptr(&owner.snapshot.midge))
                        && owner.snapshot.family_id == family_id
                })
                .map(|owner| DataReadTransaction::shared(Arc::clone(owner)))
        })
    }

    pub(super) fn statement_data_read(&self, database: &str) -> Option<DataReadTransaction> {
        CURRENT_READ.with(|current| {
            current
                .borrow()
                .as_ref()
                .filter(|owner| {
                    std::ptr::eq(self, Arc::as_ptr(&owner.snapshot.midge))
                        && owner.snapshot.database.eq_ignore_ascii_case(database)
                })
                .map(|owner| DataReadTransaction::shared(Arc::clone(owner)))
        })
    }
}

#[cfg(test)]
mod tests;
