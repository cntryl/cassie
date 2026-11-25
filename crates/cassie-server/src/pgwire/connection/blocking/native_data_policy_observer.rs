//! Test-only bridge for observing native Data commits on blocking workers.

use crate::app::Cassie;
use crate::midge::adapter::{
    clear_existing_unknown_on_next_data_submission, install_data_submission_observer,
    observe_native_commits, DataSubmission, NativeCommitObservation,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::oneshot;

#[derive(Debug)]
pub(in crate::pgwire::connection) struct Report {
    pub(in crate::pgwire::connection) data: Vec<DataSubmission>,
    pub(in crate::pgwire::connection) native: NativeCommitObservation,
    pub(in crate::pgwire::connection) worker: std::thread::ThreadId,
}

type Registration = (&'static str, oneshot::Sender<Report>);
static NEXT: OnceLock<Mutex<HashMap<usize, Registration>>> = OnceLock::new();
thread_local! {
    static DATA: RefCell<Option<Vec<DataSubmission>>> = const { RefCell::new(None) };
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
        clear_existing_unknown_on_next_data_submission();
    }
}

#[must_use = "the observation must be retained until the worker reports"]
pub(in crate::pgwire::connection) struct Observation {
    pub(in crate::pgwire::connection) report: oneshot::Receiver<Report>,
    _guard: RegistrationGuard,
}

fn record_data_submission(submission: DataSubmission) {
    DATA.with(|slot| {
        if let Some(data) = slot.borrow_mut().as_mut() {
            data.push(submission);
        }
    });
}

#[must_use = "the report identifies the worker and native commit policy"]
///
/// # Panics
/// Panics if an observation is already armed for the Cassie runtime.
pub(in crate::pgwire::connection) fn arm(cassie: &Cassie, operation: &'static str) -> Observation {
    install_data_submission_observer(record_data_submission);
    let key = Arc::as_ptr(cassie.runtime()) as usize;
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

/// Observes native commits only when a matching request was armed.
///
/// # Panics
/// Panics if a matching observation is already active on the worker thread.
pub(super) fn observe_worker<T>(cassie: &Cassie, name: &str, operation: impl FnOnce() -> T) -> T {
    let key = Arc::as_ptr(cassie.runtime()) as usize;
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
    let worker = std::thread::current().id();
    let (output, native) = observe_native_commits(operation);
    let data = DATA.with(|slot| slot.borrow_mut().take().unwrap());
    let _ = sender.send(Report {
        data,
        native,
        worker,
    });
    output
}
