//! Cassie publication coordination; Midge remains the direct storage owner.

use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

use parking_lot::{Mutex, ReentrantMutex, ReentrantMutexGuard};

use super::{CassieError, Midge};
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

mod native_access;
mod row_history;
pub(crate) use row_history::RetainedRowHistory;
pub(super) use row_history::{MutationKeys, PreparedKey};

const RECOVERY_REQUIRED: &str = "storage recovery required after an earlier unconfirmed write; this operation was not submitted; recover and reconcile before retrying";

#[derive(Debug)]
pub(crate) enum AdapterCommitError {
    Native(cntryl_midge::MidgeError),
    Cassie(CassieError),
    RecoveryRequired,
}

impl From<AdapterCommitError> for CassieError {
    fn from(error: AdapterCommitError) -> Self {
        match error {
            AdapterCommitError::Native(error) => Self::from(error),
            AdapterCommitError::Cassie(error) => error,
            AdapterCommitError::RecoveryRequired => Self::Storage(RECOVERY_REQUIRED.to_owned()),
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
                self.history_budget.get().copied().unwrap_or_else(|| {
                    crate::config::CassieRuntimeLimits::default().query_memory_budget_bytes
                }),
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
            return Err(CassieError::Storage(RECOVERY_REQUIRED.to_owned()));
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
    query: Option<QueryExecutionControls>,
    rest: Option<RestWaitControls>,
}

#[derive(Clone)]
struct RestWaitControls {
    cancellation: QueryCancellationHandle,
    deadline: Instant,
}

impl PublicationWaitControls {
    fn check_interruption(&self) -> Result<(), CassieError> {
        if let Some(controls) = &self.query {
            controls.check_interruption()?;
        }
        if let Some(controls) = &self.rest {
            if controls.cancellation.is_cancelled() {
                return Err(CassieError::QueryCancelled);
            }
            if Instant::now() >= controls.deadline {
                return Err(CassieError::DeadlineExceeded);
            }
        }
        Ok(())
    }

    fn with_query(mut self, controls: &QueryExecutionControls) -> Self {
        self.query.get_or_insert_with(|| controls.clone());
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
        controls: Option<&QueryExecutionControls>,
        operation: impl FnOnce() -> Result<(), cntryl_midge::MidgeError>,
    ) -> Result<(), AdapterCommitError> {
        self.publish_with_observer(controls, operation, |_| {})
    }

    #[cfg(test)]
    fn publish_with_observer(
        &self,
        controls: Option<&QueryExecutionControls>,
        operation: impl FnOnce() -> Result<(), cntryl_midge::MidgeError>,
        observer: impl FnMut(PublicationBoundary),
    ) -> Result<(), AdapterCommitError> {
        let controls =
            controls.map(|controls| PublicationWaitControls::default().with_query(controls));
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

    fn with_operation<T>(
        &self,
        controls: &QueryExecutionControls,
        operation: impl FnOnce() -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        let controls = CURRENT_CONTROLS
            .with(|slot| slot.borrow().clone())
            .unwrap_or_default()
            .with_query(controls);
        let _guard = self
            .lock(Some(&controls), &mut |_| {})
            .map_err(CassieError::from)?;
        operation()
    }
}

thread_local! {
    static CURRENT_CONTROLS: RefCell<Option<PublicationWaitControls>> = const { RefCell::new(None) };
}

/// A blocking call stack's current request, never a retained transaction token.
pub(crate) struct PublicationControlScope {
    previous: Option<PublicationWaitControls>,
    _thread: PhantomData<Rc<()>>,
}

impl PublicationControlScope {
    pub(crate) fn check_interruption(controls: &QueryExecutionControls) -> Result<(), CassieError> {
        CURRENT_CONTROLS.with(|slot| {
            if let Some(current) = slot.borrow().as_ref() {
                current.check_interruption()?;
            }
            controls.check_interruption()
        })
    }

    pub(crate) fn for_request(
        limits: &crate::config::CassieRuntimeLimits,
        cancellation: Option<&crate::runtime::QueryCancellationHandle>,
    ) -> Self {
        // Nested SQL/document calls keep the originating request's deadline
        // and cancellation. Standalone administrative requests get one owner.
        let controls = CURRENT_CONTROLS
            .with(|slot| slot.borrow().clone())
            .unwrap_or_else(|| {
                PublicationWaitControls::default().with_query(
                    &QueryExecutionControls::with_cancellation(
                        limits,
                        Instant::now(),
                        cancellation.cloned().unwrap_or_default(),
                    ),
                )
            });
        Self::replace(Some(controls))
    }

    pub(crate) fn enter_for_rest(
        cancellation: &QueryCancellationHandle,
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

    pub(crate) fn enter(controls: Option<&QueryExecutionControls>) -> Self {
        let current = CURRENT_CONTROLS.with(|slot| slot.borrow().clone());
        Self::replace(match controls {
            Some(controls) => Some(current.unwrap_or_default().with_query(controls)),
            None => current,
        })
    }

    /// Retire request cancellation/deadline authority only for terminal cleanup
    /// or completion after a confirmed publication. Storage quarantine remains
    /// enforced independently by every publishing path.
    pub(crate) fn terminal_completion() -> Self {
        Self::replace(None)
    }

    fn replace(controls: Option<PublicationWaitControls>) -> Self {
        Self {
            previous: CURRENT_CONTROLS.with(|slot| slot.replace(controls)),
            _thread: PhantomData,
        }
    }
}

impl Drop for PublicationControlScope {
    fn drop(&mut self) {
        drop(CURRENT_CONTROLS.with(|slot| slot.replace(self.previous.take())));
    }
}

impl Midge {
    pub(crate) fn check_storage_recovery_required(&self) -> Result<(), CassieError> {
        self.publication.ensure_recovered()
    }

    #[cfg(test)]
    pub(crate) fn observe_next_publication_wait_for_test(
        callback: impl FnOnce() + 'static,
    ) -> impl Drop {
        tests::next_wait(callback)
    }

    #[cfg(test)]
    pub(crate) fn observe_next_row_commit_for_test(callback: impl FnOnce() + 'static) -> impl Drop {
        tests::next_confirmation(callback)
    }

    #[cfg(test)]
    pub(crate) fn latch_storage_recovery_required_for_test(&self) {
        let result =
            self.publication
                .observe_native::<()>(Err(cntryl_midge::MidgeError::OutcomeUnknown(
                    "controlled unconfirmed-write boundary".to_owned(),
                )));
        assert!(result.is_err());
    }

    pub(crate) fn capture_repeatable_row_history(
        &self,
        database: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Arc<RetainedRowHistory>, CassieError> {
        self.with_database_publication(database, controls, || {
            controls.check_interruption()?;
            let family = self.database_family(database)?;
            let publication = self.publication.family(family.id());
            self.publication
                .admit_tracked_write(|| row_history::RowHistory::register(&publication))
        })
    }

    pub(crate) fn with_database_publication<T>(
        &self,
        database: &str,
        controls: &QueryExecutionControls,
        operation: impl FnOnce() -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
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
