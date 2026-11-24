//! Opaque native snapshots and their per-thread read scope.

use super::{CassieError, Midge, TransactionMode};
use cassie_core::request::{
    Interruption, InterruptionError, RequestMemory, RequestMemoryReservation,
};
use std::cell::RefCell;
use std::mem::size_of;
use std::sync::Arc;
use std::time::Instant;

mod materialized_outputs;

thread_local! {
    static CURRENT_READ: RefCell<Option<ScopedNativeRead>> = const { RefCell::new(None) };
}

#[derive(Clone)]
struct ScopedNativeRead {
    snapshot: Arc<NativeStatementSnapshot>,
    checkpoint_storage: Option<Arc<super::CheckpointStorageSnapshot>>,
}

/// The concrete request boundary accepted by storage operations.
pub struct StorageRequestContext<'a> {
    snapshot: Option<&'a NativeStatementSnapshot>,
    interruption: Interruption<'a>,
    memory: &'a RequestMemory,
    scan_control: Option<&'a super::QueryScanControlScope>,
}

impl<'a> StorageRequestContext<'a> {
    pub const fn new(
        snapshot: Option<&'a NativeStatementSnapshot>,
        interruption: Interruption<'a>,
        memory: &'a RequestMemory,
    ) -> Self {
        Self {
            snapshot,
            interruption,
            memory,
            scan_control: None,
        }
    }

    #[must_use]
    pub const fn with_scan_control(
        mut self,
        scan_control: Option<&'a super::QueryScanControlScope>,
    ) -> Self {
        self.scan_control = scan_control;
        self
    }

    pub(super) fn should_cancel_controlled_query_scan(&self) -> bool {
        self.scan_control
            .is_some_and(super::QueryScanControlScope::should_cancel)
    }

    #[must_use]
    pub const fn snapshot(&self) -> Option<&'a NativeStatementSnapshot> {
        self.snapshot
    }

    #[must_use]
    pub const fn memory(&self) -> &'a RequestMemory {
        self.memory
    }

    #[must_use]
    pub const fn deadline(&self) -> Option<Instant> {
        self.interruption.deadline()
    }

    #[must_use]
    pub fn is_canceled(&self) -> bool {
        self.interruption.cancellation().is_canceled()
    }

    #[must_use]
    pub fn is_timed_out(&self) -> bool {
        self.deadline().is_some_and(|deadline| Instant::now() >= deadline)
    }

    #[must_use]
    pub fn current_query_memory_bytes(&self) -> usize {
        self.memory.current_bytes()
    }

    #[must_use]
    pub fn query_memory_budget_bytes(&self) -> usize {
        self.memory.budget()
    }

    pub fn check_interruption(&self) -> Result<(), CassieError> {
        self.interruption.check().map_err(|error| match error {
            InterruptionError::Canceled => CassieError::QueryCanceled,
            InterruptionError::DeadlineExceeded => CassieError::DeadlineExceeded,
        })
    }

    pub fn reserve(&self, bytes: usize) -> Result<RequestMemoryReservation, CassieError> {
        self.memory.reserve(bytes).map_err(native_memory_limit_error)
    }

    pub fn reserve_query_memory(&self, bytes: usize) -> Result<RequestMemoryReservation, CassieError> {
        self.reserve(bytes)
    }

    pub fn grow(
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

/// Opaque native read authority retained independently from engine session state.
pub struct NativeStatementSnapshot {
    tx: super::AdapterTransaction,
    midge: Arc<Midge>,
    database: String,
    family_id: cntryl_midge::ColumnFamilyId,
    data_epoch: u64,
    descriptor_authority: DescriptorAuthority,
    memory: RequestMemoryReservation,
}

impl NativeStatementSnapshot {
    pub fn capture(
        midge: &Arc<Midge>,
        database: &str,
        context: &StorageRequestContext<'_>,
        current_catalog: bool,
    ) -> Result<Arc<Self>, CassieError> {
        context.check_interruption()?;
        let bytes = size_of::<Self>()
            .checked_add(2 * size_of::<usize>())
            .and_then(|bytes| bytes.checked_add(database.len()))
            .ok_or_else(|| {
                CassieError::ResourceLimit("statement read owner size overflow".to_string())
            })?;
        let memory = context.reserve(bytes)?;
        let family = midge.database_family(database)?;
        let family_id = family.id();
        let tx = if current_catalog {
            midge.begin_committed_read_adapter_tx(family_id)?
        } else {
            midge.begin_adapter_tx(family_id, TransactionMode::ReadOnly)?
        };
        drop(family);
        let epoch_scratch = context.reserve(super::key_encoding::data_epoch_key_scratch_bytes())?;
        let data_epoch = Midge::load_data_epoch_from_tx(&tx)?;
        drop(epoch_scratch);
        Ok(Arc::new(Self {
            tx,
            midge: Arc::clone(midge),
            database: database.to_owned(),
            family_id,
            data_epoch,
            descriptor_authority: if current_catalog {
                DescriptorAuthority::CurrentCatalog
            } else {
                DescriptorAuthority::StatementSnapshot
            },
            memory,
        }))
    }

    #[must_use]
    pub fn data_epoch(&self) -> u64 {
        self.data_epoch
    }

    #[must_use]
    pub fn matches(&self, midge: &Arc<Midge>, database: &str) -> bool {
        Arc::ptr_eq(&self.midge, midge) && self.database.eq_ignore_ascii_case(database)
    }

    pub fn matches_current_family(
        &self,
        midge: &Arc<Midge>,
        database: &str,
    ) -> Result<bool, CassieError> {
        Ok(self.matches(midge, database)
            && midge.database_family(database)?.id() == self.family_id)
    }

    #[must_use]
    pub fn accounted_bytes(&self) -> usize {
        self.memory.bytes()
    }
}

/// Installs a native snapshot in the current thread for existing storage reads.
pub struct NativeStatementReadScope {
    previous: Option<ScopedNativeRead>,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl NativeStatementReadScope {
    pub fn enter(
        snapshot: Option<&Arc<NativeStatementSnapshot>>,
        checkpoint_storage: Option<Arc<super::CheckpointStorageSnapshot>>,
    ) -> Self {
        let next = snapshot.map(|snapshot| ScopedNativeRead {
            snapshot: Arc::clone(snapshot),
            checkpoint_storage,
        });
        let previous = CURRENT_READ.with(|current| current.replace(next));
        Self {
            previous,
            _thread: std::marker::PhantomData,
        }
    }
}

impl Drop for NativeStatementReadScope {
    fn drop(&mut self) {
        CURRENT_READ.with(|current| current.replace(self.previous.take()));
    }
}

/// A read transaction is either freshly owned or shares an installed snapshot.
pub(super) struct DataReadTransaction {
    fresh: Option<super::AdapterTransaction>,
    shared: Option<ScopedNativeRead>,
}

impl DataReadTransaction {
    pub(super) fn checkpoint_storage(
        &self,
    ) -> Option<&Arc<super::CheckpointStorageSnapshot>> {
        self.shared.as_ref()?.checkpoint_storage.as_ref()
    }

    pub(super) fn fresh(tx: super::AdapterTransaction) -> Self {
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
    type Target = super::AdapterTransaction;

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
