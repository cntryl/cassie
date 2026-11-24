//! Test-only paired worker/native-Data observation; never product telemetry.
use crate::app::Cassie;
use crate::midge::adapter::{observe_native_commits, NativeCommitObservation};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::ThreadId;
use tokio::sync::oneshot;

type Registration = (&'static str, oneshot::Sender<Report>);
static NEXT: OnceLock<Mutex<HashMap<usize, Registration>>> = OnceLock::new();
thread_local! {
    static DATA: RefCell<Option<Vec<DataSubmission>>> = const { RefCell::new(None) };
    static UNKNOWN_ON_DATA: Cell<bool> = const { Cell::new(false) };
}

#[derive(Debug)]
pub(crate) struct DataSubmission {
    pub(crate) policy: &'static str,
    pub(crate) succeeded: bool,
    pub(crate) outcome_unknown: bool,
}

#[derive(Debug)]
pub(crate) struct Report {
    pub(crate) worker: ThreadId,
    pub(crate) data: Vec<DataSubmission>,
    pub(crate) native: NativeCommitObservation,
}

pub(crate) struct Observation {
    pub(crate) report: oneshot::Receiver<Report>,
    _guard: RegistrationGuard,
}

struct RegistrationGuard(usize);
impl Drop for RegistrationGuard {
    fn drop(&mut self) {
        if let Some(registry) = NEXT.get() {
            registry.lock().unwrap().remove(&self.0);
        }
    }
}

struct DataGuard;
impl Drop for DataGuard {
    fn drop(&mut self) {
        DATA.with(|slot| slot.borrow_mut().take());
        UNKNOWN_ON_DATA.set(false);
    }
}

pub(crate) fn arm(cassie: &Cassie, operation: &'static str) -> Observation {
    let key = Arc::as_ptr(&cassie.runtime) as usize;
    let (sender, report) = oneshot::channel();
    assert!(NEXT
        .get_or_init(Mutex::default)
        .lock()
        .unwrap()
        .insert(key, (operation, sender))
        .is_none());
    Observation {
        report,
        _guard: RegistrationGuard(key),
    }
}

pub(crate) fn observe_worker<T>(cassie: &Cassie, name: &str, operation: impl FnOnce() -> T) -> T {
    let key = Arc::as_ptr(&cassie.runtime) as usize;
    let sender = NEXT.get().and_then(|registry| {
        let mut registry = registry.lock().unwrap();
        if registry
            .get(&key)
            .is_some_and(|(operation, _)| *operation == name)
        {
            registry.remove(&key).map(|(_, sender)| sender)
        } else {
            None
        }
    });
    let Some(sender) = sender else {
        return operation();
    };
    DATA.with(|slot| assert!(slot.replace(Some(Vec::new())).is_none()));
    let _guard = DataGuard;
    let (output, native) = observe_native_commits(operation);
    let data = DATA.with(|slot| slot.borrow_mut().take().unwrap());
    let _ = sender.send(Report {
        worker: std::thread::current().id(),
        data,
        native,
    });
    output
}

pub(crate) fn current_data_submission_count() -> Option<usize> {
    DATA.with(|slot| slot.borrow().as_ref().map(Vec::len))
}

/// Target the existing adapter-response simulation at Data, not an earlier
/// schema epoch commit. The native result is not replaced by this observer.
pub(crate) fn arm_existing_unknown_on_next_data_submission() {
    DATA.with(|slot| assert!(slot.borrow().is_some(), "requires an active observer"));
    UNKNOWN_ON_DATA.set(true);
}

pub(super) fn observe_data_submission<T>(
    options: cntryl_midge::WriteOptions,
    submit: impl FnOnce() -> Result<T, cntryl_midge::MidgeError>,
) -> Result<T, cntryl_midge::MidgeError> {
    // Invoked inside the owning row-publication callback, after all staging /
    // suppression branches and publication permission. Record the real result.
    if UNKNOWN_ON_DATA.replace(false) {
        super::inject_native_commit_unknown_after_success();
    }
    let result = submit();
    DATA.with(|slot| {
        if let Some(data) = slot.borrow_mut().as_mut() {
            let policy = match options.policy() {
                cntryl_midge::DurabilityPolicy::Sync => "local_sync",
                cntryl_midge::DurabilityPolicy::Buffered => "local_buffered",
                cntryl_midge::DurabilityPolicy::CloudAsync => "cloud_async",
                cntryl_midge::DurabilityPolicy::CloudStrict => "cloud_strict",
                cntryl_midge::DurabilityPolicy::BestEffort => "best_effort",
            };
            data.push(DataSubmission {
                policy,
                succeeded: result.is_ok(),
                outcome_unknown: matches!(
                    &result,
                    Err(cntryl_midge::MidgeError::OutcomeUnknown(_))
                ),
            });
        }
    });
    result
}
