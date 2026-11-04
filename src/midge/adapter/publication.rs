//! Cassie publication coordination; Midge remains the direct storage owner.

use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use parking_lot::{Mutex, ReentrantMutex, ReentrantMutexGuard};

use super::{CassieError, Midge};
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

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
        });
        families.insert(family, Arc::downgrade(&gate));
        gate
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
        self.publish_with_wait_observer(controls.as_ref(), operation, |_| {})
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
        mut observer: impl FnMut(PublicationBoundary),
    ) -> Result<(), AdapterCommitError> {
        let _guard = self.lock(controls, &mut observer)?;
        let result = operation();
        if matches!(&result, Err(cntryl_midge::MidgeError::OutcomeUnknown(_))) {
            // Midge may still publish after an unknown response. Latch while
            // protected, before a waiter can enter another publication window.
            self.recovery_required.store(true, Ordering::Release);
            observer(PublicationBoundary::RecoveryLatched);
        }
        result.map_err(AdapterCommitError::Native)
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

    fn replace(controls: Option<PublicationWaitControls>) -> Self {
        Self {
            previous: CURRENT_CONTROLS.with(|slot| slot.replace(controls)),
            _thread: PhantomData,
        }
    }
}

impl Drop for PublicationControlScope {
    fn drop(&mut self) {
        CURRENT_CONTROLS.with(|slot| slot.replace(self.previous.take()));
    }
}

impl Midge {
    pub(crate) fn check_storage_recovery_required(&self) -> Result<(), CassieError> {
        self.publication.ensure_recovered()
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
mod tests;
