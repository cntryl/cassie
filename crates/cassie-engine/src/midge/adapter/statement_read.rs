//! Private same-database statement Data ownership. Write transactions remain fresh.

use super::{CassieError, Midge, TransactionMode};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use cassie_core::request::{
    Interruption, InterruptionError, RequestMemory, RequestMemoryReservation,
};
use std::cell::RefCell;
use std::mem::size_of;
use std::sync::Arc;

mod materialized_outputs;

thread_local! {
    static CURRENT_READ: RefCell<Option<ScopedNativeRead>> = const { RefCell::new(None) };
}

#[derive(Clone)]
struct ScopedNativeRead {
    snapshot: Arc<NativeStatementSnapshot>,
    checkpoint_storage: Option<Arc<super::CheckpointStorageSnapshot>>,
}

/// Concrete storage request boundary. It carries only native snapshot authority,
/// live interruption state, and the request-wide memory reservation source.
pub(crate) struct NativeStatementReadContext<'a> {
    snapshot: Option<&'a NativeStatementSnapshot>,
    interruption: Interruption<'a>,
    memory: &'a RequestMemory,
}

impl<'a> NativeStatementReadContext<'a> {
    pub(crate) const fn new(
        snapshot: Option<&'a NativeStatementSnapshot>,
        interruption: Interruption<'a>,
        memory: &'a RequestMemory,
    ) -> Self {
        Self {
            snapshot,
            interruption,
            memory,
        }
    }

    pub(crate) const fn snapshot(&self) -> Option<&'a NativeStatementSnapshot> {
        self.snapshot
    }

    pub(crate) fn check_interruption(&self) -> Result<(), CassieError> {
        self.interruption.check().map_err(|error| match error {
            InterruptionError::Canceled => CassieError::QueryCanceled,
            InterruptionError::DeadlineExceeded => CassieError::DeadlineExceeded,
        })
    }

    pub(crate) fn reserve(&self, bytes: usize) -> Result<RequestMemoryReservation, CassieError> {
        self.memory
            .reserve(bytes)
            .map_err(native_memory_limit_error)
    }

    pub(crate) fn grow(
        &self,
        reservation: &mut RequestMemoryReservation,
        bytes: usize,
    ) -> Result<(), CassieError> {
        reservation
            .try_grow(bytes)
            .map_err(native_memory_limit_error)
    }
}

fn native_memory_limit_error(error: cassie_core::request::MemoryBudgetError) -> CassieError {
    CassieError::ResourceLimit(format!(
        "query memory budget exceeded: {} > {}",
        error.requested, error.budget
    ))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DescriptorAuthority {
    StatementSnapshot,
    CurrentCatalog,
}

/// Opaque native read authority retained independently from the engine's
/// command overlay. The storage relocation can move this owner without moving
/// `StatementDataRead` or the session COW state it wraps.
pub(crate) struct NativeStatementSnapshot {
    // Release the snapshot before its engine owner, then its admitted metadata.
    tx: crate::midge::adapter::AdapterTransaction,
    midge: Arc<Midge>,
    database: String,
    family_id: cntryl_midge::ColumnFamilyId,
    data_epoch: u64,
    descriptor_authority: DescriptorAuthority,
    memory: RequestMemoryReservation,
}

pub(crate) struct StatementDataRead {
    snapshot: Arc<NativeStatementSnapshot>,
    overlay: Option<Arc<crate::app::StatementOverlay>>,
    private_outputs: Option<materialized_outputs::MaterializedReads>,
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

    /// Additional per-command retained backing; the immutable transaction core
    /// is shared with the transaction owner and is admitted there once.
    pub(crate) fn command_accounted_bytes(&self) -> Result<usize, CassieError> {
        let command = crate::executor::retained_memory::add(
            self.memory.bytes(),
            self.overlay
                .as_ref()
                .map_or(0, |owner| owner.accounted_bytes()),
        )?;
        crate::executor::retained_memory::add(
            command,
            self.private_outputs.as_ref().map_or(
                Ok(0),
                materialized_outputs::MaterializedReads::accounted_bytes,
            )?,
        )
    }

    pub(crate) fn private_owner_accounted_bytes(&self) -> Result<usize, CassieError> {
        crate::executor::retained_memory::add(self.memory.bytes(), self.snapshot.memory.bytes())
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
        let context = NativeStatementReadContext::new(
            Some(&retained.snapshot),
            controls.interruption(),
            controls.request_memory(),
        );
        context.check_interruption()?;
        let native_snapshot = context.snapshot().ok_or_else(|| {
            CassieError::Execution("retained native statement snapshot is missing".to_owned())
        })?;
        if !std::ptr::eq(native_snapshot, Arc::as_ptr(&retained.snapshot)) {
            return Err(CassieError::Execution(
                "retained native statement snapshot changed while rebinding".to_owned(),
            ));
        }
        let memory = controls.reserve_query_memory(size_of::<Self>() + 2 * size_of::<usize>())?;
        Ok(Arc::new(Self {
            snapshot: Arc::clone(&retained.snapshot),
            private_outputs: retained.private_outputs.clone(),
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
        let context = NativeStatementReadContext::new(
            None,
            controls.interruption(),
            controls.request_memory(),
        );
        context.check_interruption()?;
        // Midge owns transaction-internal snapshot retention. Cassie owns this
        // Arc allocation, its inline transaction and copied database identity.
        let bytes = size_of::<NativeStatementSnapshot>()
            .checked_add(2 * size_of::<usize>())
            .and_then(|bytes| bytes.checked_add(database.len()))
            .ok_or_else(|| {
                CassieError::ResourceLimit("statement read owner size overflow".to_string())
            })?;
        let memory = context.reserve(bytes)?;
        let command_memory =
            controls.reserve_query_memory(size_of::<Self>() + 2 * size_of::<usize>())?;
        let family = midge.database_family(database)?;
        let family_id = family.id();
        let tx = if descriptor_authority == DescriptorAuthority::CurrentCatalog {
            midge.begin_committed_read_adapter_tx(family_id)?
        } else {
            midge.begin_adapter_tx(family_id, TransactionMode::ReadOnly)?
        };
        // The temporary family handle does not become statement-owned backing.
        drop(family);
        let epoch_scratch = context.reserve(super::key_encoding::data_epoch_key_scratch_bytes())?;
        let data_epoch = Midge::load_data_epoch_from_tx(&tx)?;
        drop(epoch_scratch);
        let snapshot = Arc::new(NativeStatementSnapshot {
            tx,
            midge: Arc::clone(midge),
            database: database.to_owned(),
            family_id,
            data_epoch,
            descriptor_authority,
            memory,
        });
        Ok(Arc::new(Self {
            snapshot,
            private_outputs: None,
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
    shared: Option<ScopedNativeRead>,
}

impl DataReadTransaction {
    pub(super) fn checkpoint_storage(&self) -> Option<&Arc<super::CheckpointStorageSnapshot>> {
        self.shared.as_ref()?.checkpoint_storage.as_ref()
    }

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

    fn shared(context: ScopedNativeRead) -> Self {
        Self {
            fresh: None,
            shared: Some(context),
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
    previous: Option<ScopedNativeRead>,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl StatementReadScope {
    pub(crate) fn enter(owner: Option<&Arc<StatementDataRead>>) -> Self {
        let next = owner.map(|owner| ScopedNativeRead {
            snapshot: Arc::clone(&owner.snapshot),
            checkpoint_storage: owner
                .overlay()
                .and_then(|overlay| overlay.checkpoint_storage().cloned()),
        });
        let previous = CURRENT_READ.with(|current| current.replace(next));
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
                .map(|owner| DataReadTransaction::shared(owner.clone()))
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
                .map(|owner| DataReadTransaction::shared(owner.clone()))
        })
    }
}

#[cfg(test)]
mod tests;
