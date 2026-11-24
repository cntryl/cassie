//! Cassie publication coordination; Midge remains the direct storage owner.

use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

use parking_lot::{Mutex, ReentrantMutex, ReentrantMutexGuard};

use super::{CassieError, Midge, NativeRequestControls, StorageRequestContext};
use cassie_core::request::{Interruption, RequestCancellation};

mod native_access;
mod row_history;
pub use row_history::RetainedRowHistory;
pub(super) use row_history::{MutationKeys, PreparedKey};

const RECOVERY_REQUIRED: &str = "storage recovery required after an earlier unconfirmed write; this operation was not submitted; recover and reconcile before retrying";

#[derive(Debug)]
pub enum AdapterCommitError {
    Native(cntryl_midge::MidgeError),
    Cassie(CassieError),
    RecoveryRequired,
}

impl From<AdapterCommitError> for CassieError {
    fn from(error: AdapterCommitError) -> Self {
        match error {
            AdapterCommitError::Native(error) => Self::from(error),
            AdapterCommitError::Cassie(error) => error,
            AdapterCommitError::RecoveryRequired => {
                Self::RecoveryRequired(RECOVERY_REQUIRED.to_owned())
            }
        }
    }
}

#[derive(Default)]
pub(super) struct PublicationCoordinator {
    families: Mutex<HashMap<cntryl_midge::ColumnFamilyId, Weak<DatabasePublication>>>,
    recovery_required: Arc<AtomicBool>,
    native_access: native_access::NativeAccess,
    history_budget: OnceLock<usize>,
}

impl PublicationCoordinator {
    pub(super) fn family(&self, family: cntryl_midge::ColumnFamilyId) -> Arc<DatabasePublication> {
        let mut families = self.families.lock();
        families.retain(|_, gate| gate.strong_count() > 0);
        if let Some(existing) = families.get(&family).and_then(Weak::upgrade) {
            return existing;
        }
        let gate = Arc::new(DatabasePublication {
            mutex: ReentrantMutex::new(()),
            recovery_required: Arc::clone(&self.recovery_required),
            row_history: row_history::RowHistory::new(
                self.history_budget
                    .get()
                    .copied()
                    .unwrap_or(cassie_core::request::DEFAULT_QUERY_MEMORY_BUDGET_BYTES),
            ),
        });
        families.insert(family, Arc::downgrade(&gate));
        gate
    }

    pub(super) fn open_native<T>(
        &self,
        mode: cntryl_midge::TransactionMode,
        open: impl FnOnce() -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        self.native_access.native(mode, open)
    }

    pub(super) fn admit_tracked_write<T>(
        &self,
        register: impl FnOnce() -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        self.native_access.tracked(register)
    }

    pub(super) fn configure_history_budget(&self, bytes: usize) -> Result<(), CassieError> {
        if self
            .history_budget
            .get()
            .is_some_and(|current| *current != bytes)
            || !self.families.lock().is_empty()
        {
            return Err(CassieError::Unsupported(
                "row-history budget must be configured before storage admission".to_owned(),
            ));
        }
        let _ = self.history_budget.set(bytes);
        Ok(())
    }

    pub(super) fn ensure_recovered(&self) -> Result<(), CassieError> {
        if self.recovery_required.load(Ordering::Acquire) {
            return Err(CassieError::RecoveryRequired(RECOVERY_REQUIRED.to_owned()));
        }
        Ok(())
    }

    pub(super) fn observe_native<T>(
        &self,
        result: Result<T, cntryl_midge::MidgeError>,
    ) -> Result<T, cntryl_midge::MidgeError> {
        if matches!(&result, Err(cntryl_midge::MidgeError::OutcomeUnknown(_))) {
            self.recovery_required.store(true, Ordering::Release);
        }
        result
    }
}

pub(super) struct DatabasePublication {
    mutex: ReentrantMutex<()>,
    recovery_required: Arc<AtomicBool>,
    pub(super) row_history: row_history::RowHistory,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PublicationBoundary {
    WaitingForGate,
    RecoveryLatched,
}

#[derive(Clone, Default)]
struct PublicationWaitControls {
    query: Option<NativeRequestControls>,
    rest: Option<RestWaitControls>,
    /// Explicit replay cancellation handles, combined with ambient authority.
    replay: Vec<RequestCancellation>,
}

#[derive(Clone)]
struct RestWaitControls {
    cancellation: RequestCancellation,
    deadline: Instant,
}

impl PublicationWaitControls {
    fn check_interruption(&self) -> Result<(), CassieError> {
        if let Some(controls) = &self.query {
            controls
                .context()
                .check_interruption()?;
        }
        if let Some(controls) = &self.rest {
            if controls.cancellation.is_canceled() {
                return Err(CassieError::QueryCanceled);
            }
            if Instant::now() >= controls.deadline {
                return Err(CassieError::DeadlineExceeded);
            }
        }
        if self.replay.iter().any(RequestCancellation::is_canceled) {
            return Err(CassieError::QueryCanceled);
        }
        Ok(())
    }

    fn with_interruption(
        mut self,
        interruption: Interruption<'_>,
        memory: &cassie_core::request::RequestMemory,
    ) -> Self {
        self.query.get_or_insert_with(|| {
            NativeRequestControls::new(interruption, memory.clone())
        });
        self
    }
}

impl DatabasePublication {
    fn lock(
        &self,
        controls: Option<&PublicationWaitControls>,
        observer: &mut impl FnMut(PublicationBoundary),
    ) -> Result<ReentrantMutexGuard<'_, ()>, AdapterCommitError> {
        let guard = if let Some(controls) = controls {
            loop {
                controls
                    .check_interruption()
                    .map_err(AdapterCommitError::Cassie)?;
                if let Some(guard) = self.mutex.try_lock_for(Duration::from_millis(2)) {
                    break guard;
                }
                observer(PublicationBoundary::WaitingForGate);
            }
        } else {
            self.mutex.lock()
        };
        if let Some(controls) = controls {
            controls
                .check_interruption()
                .map_err(AdapterCommitError::Cassie)?;
        }
        if self.recovery_required.load(Ordering::Acquire) {
            return Err(AdapterCommitError::RecoveryRequired);
        }
        Ok(guard)
    }

    pub(super) fn publish_current(
        &self,
        operation: impl FnOnce() -> Result<(), cntryl_midge::MidgeError>,
    ) -> Result<(), AdapterCommitError> {
        let controls = CURRENT_CONTROLS.with(|slot| slot.borrow().clone());
        self.publish_with_wait_observer(controls.as_ref(), operation, observe_publication_boundary)
    }

    pub(super) fn publish_rows_current(
        &self,
        keys: MutationKeys,
        operation: impl FnOnce() -> Result<(), cntryl_midge::MidgeError>,
    ) -> Result<(), AdapterCommitError> {
        let controls = CURRENT_CONTROLS.with(|slot| slot.borrow().clone());
        #[cfg(test)]
        let row_write = keys.has_keys();
        self.publish_controlled(
            controls.as_ref(),
            || {
                let result = self.row_history.publish(keys, operation);
                #[cfg(test)]
                if row_write && result.is_ok() {
                    tests::observe_confirmation();
                }
                result
            },
            observe_publication_boundary,
        )
    }

    #[cfg(test)]
    fn publish_with(
        &self,
        controls: Option<&StorageRequestContext<'_>>,
        operation: impl FnOnce() -> Result<(), cntryl_midge::MidgeError>,
    ) -> Result<(), AdapterCommitError> {
        self.publish_with_observer(controls, operation, |_| {})
    }

    #[cfg(test)]
    fn publish_with_observer(
        &self,
        controls: Option<&StorageRequestContext<'_>>,
        operation: impl FnOnce() -> Result<(), cntryl_midge::MidgeError>,
        observer: impl FnMut(PublicationBoundary),
    ) -> Result<(), AdapterCommitError> {
        let controls = controls.map(|controls| {
            PublicationWaitControls::default()
                .with_interruption(controls.interruption(), controls.memory())
        });
        self.publish_with_wait_observer(controls.as_ref(), operation, observer)
    }

    fn publish_with_wait_observer(
        &self,
        controls: Option<&PublicationWaitControls>,
        operation: impl FnOnce() -> Result<(), cntryl_midge::MidgeError>,
        observer: impl FnMut(PublicationBoundary),
    ) -> Result<(), AdapterCommitError> {
        self.publish_controlled(
            controls,
            || operation().map_err(AdapterCommitError::Native),
            observer,
        )
    }

    fn publish_controlled(
        &self,
        controls: Option<&PublicationWaitControls>,
        operation: impl FnOnce() -> Result<(), AdapterCommitError>,
        mut observer: impl FnMut(PublicationBoundary),
    ) -> Result<(), AdapterCommitError> {
        let _guard = self.lock(controls, &mut observer)?;
        let result = operation();
        if matches!(
            &result,
            Err(AdapterCommitError::Native(
                cntryl_midge::MidgeError::OutcomeUnknown(_)
            ))
        ) {
            // Midge may still publish after an unknown response. Latch while
            // protected, before a waiter can enter another publication window.
            self.recovery_required.store(true, Ordering::Release);
            observer(PublicationBoundary::RecoveryLatched);
        }
        result
    }

    pub(super) fn with_current_operation<T>(
        &self,
        operation: impl FnOnce() -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        let controls = CURRENT_CONTROLS.with(|slot| slot.borrow().clone());
        let _guard = self
            .lock(controls.as_ref(), &mut observe_publication_boundary)
            .map_err(CassieError::from)?;
        operation()
    }

    fn with_operation<T, E>(
        &self,
        controls: &StorageRequestContext<'_>,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<CassieError>,
    {
        let controls = CURRENT_CONTROLS
            .with(|slot| slot.borrow().clone())
            .unwrap_or_default()
            .with_interruption(controls.interruption(), controls.memory());
        let _guard = self
            .lock(Some(&controls), &mut |_| {})
            .map_err(CassieError::from)
            .map_err(E::from)?;
        operation()
    }
}

thread_local! {
    static CURRENT_CONTROLS: RefCell<Option<PublicationWaitControls>> = const { RefCell::new(None) };
}

/// A blocking call stack's current request, never a retained transaction token.
#[doc(hidden)]
pub struct NativePublicationControlScope {
    previous: Option<PublicationWaitControls>,
    _thread: PhantomData<Rc<()>>,
}

impl NativePublicationControlScope {
    #[doc(hidden)]
    pub fn check_interruption(controls: &StorageRequestContext<'_>) -> Result<(), CassieError> {
        CURRENT_CONTROLS.with(|slot| {
            if let Some(current) = slot.borrow().as_ref() {
                current.check_interruption()?;
            }
            controls.check_interruption()
        })
    }

    #[doc(hidden)]
    pub fn enter_for_rest(
        cancellation: &RequestCancellation,
        deadline: Instant,
    ) -> Self {
        let mut controls = CURRENT_CONTROLS
            .with(|slot| slot.borrow().clone())
            .unwrap_or_default();
        controls.rest.get_or_insert_with(|| RestWaitControls {
            cancellation: cancellation.clone(),
            deadline,
        });
        Self::replace(Some(controls))
    }

    /// Adds an explicit replay cancellation handle to the ambient authority.
    #[doc(hidden)]
    pub fn enter_for_replay(cancellation: &RequestCancellation) -> Self {
        let mut controls = CURRENT_CONTROLS
            .with(|slot| slot.borrow().clone())
            .unwrap_or_default();
        controls.replay.push(cancellation.clone());
        Self::replace(Some(controls))
    }

    /// Checks the current call stack's combined cancellation and deadline.
    #[doc(hidden)]
    pub fn check_current() -> Result<(), CassieError> {
        CURRENT_CONTROLS.with(|slot| {
            slot.borrow()
                .as_ref()
                .map_or(Ok(()), PublicationWaitControls::check_interruption)
        })
    }

    pub(crate) fn current_request_controls() -> Option<NativeRequestControls> {
        CURRENT_CONTROLS.with(|slot| slot.borrow().as_ref().and_then(|value| value.query.clone()))
    }

    #[doc(hidden)]
    pub fn enter(controls: Option<&StorageRequestContext<'_>>) -> Self {
        let current = CURRENT_CONTROLS.with(|slot| slot.borrow().clone());
        Self::replace(match controls {
            Some(controls) => Some(
                current
                    .unwrap_or_default()
                    .with_interruption(controls.interruption(), controls.memory()),
            ),
            None => current,
        })
    }

    /// Retire request cancellation/deadline authority only for terminal cleanup
    /// or completion after a confirmed publication. Storage quarantine remains
    /// enforced independently by every publishing path.
    #[doc(hidden)]
    pub fn terminal_completion() -> Self {
        Self::replace(None)
    }

    fn replace(controls: Option<PublicationWaitControls>) -> Self {
        Self {
            previous: CURRENT_CONTROLS.with(|slot| slot.replace(controls)),
            _thread: PhantomData,
        }
    }
}

impl Drop for NativePublicationControlScope {
    fn drop(&mut self) {
        drop(CURRENT_CONTROLS.with(|slot| slot.replace(self.previous.take())));
    }
}

impl Midge {
    pub fn check_storage_recovery_required(&self) -> Result<(), CassieError> {
        self.publication.ensure_recovered()
    }

    #[cfg(test)]
    pub fn observe_next_publication_wait_for_test(
        callback: impl FnOnce() + 'static,
    ) -> impl Drop {
        tests::next_wait(callback)
    }

    #[cfg(test)]
    pub fn observe_next_row_commit_for_test(callback: impl FnOnce() + 'static) -> impl Drop {
        tests::next_confirmation(callback)
    }

    #[cfg(any(test, feature = "storage-test-hooks"))]
    #[doc(hidden)]
    pub fn latch_storage_recovery_required_for_test(&self) {
        let result =
            self.publication
                .observe_native::<()>(Err(cntryl_midge::MidgeError::OutcomeUnknown(
                    "controlled unconfirmed-write boundary".to_owned(),
                )));
        assert!(result.is_err());
    }

    pub fn capture_repeatable_row_history(
        &self,
        database: &str,
        controls: &StorageRequestContext<'_>,
    ) -> Result<Arc<RetainedRowHistory>, CassieError> {
        self.with_database_publication(database, controls, || {
            controls.check_interruption()?;
            let family = self.database_family(database)?;
            let publication = self.publication.family(family.id());
            self.publication
                .admit_tracked_write(|| row_history::RowHistory::register(&publication))
        })
    }

    pub fn with_database_publication<T, E>(
        &self,
        database: &str,
        controls: &StorageRequestContext<'_>,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<CassieError>,
    {
        let family = self.database_family(database)?;
        let gate = self.publication.family(family.id());
        gate.with_operation(controls, operation)
    }
}

#[cfg(test)]
fn observe_publication_boundary(boundary: PublicationBoundary) {
    tests::observe_publication(boundary);
}
#[cfg(not(test))]
fn observe_publication_boundary(_: PublicationBoundary) {}

#[cfg(test)]
mod tests;
