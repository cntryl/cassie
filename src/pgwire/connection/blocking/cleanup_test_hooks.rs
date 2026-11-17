//! Owned-worker cleanup boundaries; no scheduling sleeps or native-engine hooks.

use super::{Arc, Cassie};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread::ThreadId;
use std::time::Duration;
use tokio::sync::oneshot;

pub(in crate::pgwire::connection) struct Observation {
    pub(in crate::pgwire::connection) worker: ThreadId,
    pub(in crate::pgwire::connection) recovery_allowed: bool,
    pub(in crate::pgwire::connection) connection_admission:
        Option<std::sync::Weak<tokio::sync::Semaphore>>,
}

pub(in crate::pgwire::connection) struct Control {
    pub(in crate::pgwire::connection) entered: oneshot::Receiver<Observation>,
    pub(in crate::pgwire::connection) resume: mpsc::SyncSender<()>,
    pub(in crate::pgwire::connection) completed: oneshot::Receiver<()>,
    pub(in crate::pgwire::connection) calls: Arc<AtomicUsize>,
    _guard: Guard,
}

struct Guard(usize);

impl Drop for Guard {
    fn drop(&mut self) {
        registry().lock().expect("cleanup probes").remove(&self.0);
    }
}

pub(in crate::pgwire::connection) struct Probe {
    entered: Option<oneshot::Sender<Observation>>,
    resume: mpsc::Receiver<()>,
    completed: oneshot::Sender<()>,
    calls: Arc<AtomicUsize>,
}

struct Registered {
    probe: Option<Probe>,
    abort_queued: bool,
}

fn registry() -> &'static Mutex<HashMap<usize, Registered>> {
    static PROBES: OnceLock<Mutex<HashMap<usize, Registered>>> = OnceLock::new();
    PROBES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(in crate::pgwire::connection) fn key(cassie: &Cassie) -> usize {
    Arc::as_ptr(&cassie.runtime).cast::<()>() as usize
}

pub(in crate::pgwire::connection) fn install(cassie: &Cassie, abort_queued: bool) -> Control {
    let (entered_tx, entered) = oneshot::channel();
    let (resume, resume_rx) = mpsc::sync_channel(1);
    let (completed_tx, completed) = oneshot::channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let key = key(cassie);
    let probe = Probe {
        entered: Some(entered_tx),
        resume: resume_rx,
        completed: completed_tx,
        calls: Arc::clone(&calls),
    };
    assert!(registry()
        .lock()
        .expect("cleanup probes")
        .insert(
            key,
            Registered {
                probe: Some(probe),
                abort_queued
            }
        )
        .is_none());
    Control {
        entered,
        resume,
        completed,
        calls,
        _guard: Guard(key),
    }
}

pub(in crate::pgwire::connection) fn take(key: usize) -> Option<Probe> {
    registry()
        .lock()
        .expect("cleanup probes")
        .get_mut(&key)
        .and_then(|entry| entry.probe.take())
}

pub(in crate::pgwire::connection) fn abort_before_start(key: usize) -> bool {
    registry()
        .lock()
        .expect("cleanup probes")
        .get_mut(&key)
        .is_some_and(|entry| std::mem::take(&mut entry.abort_queued))
}

impl Probe {
    pub(in crate::pgwire::connection) fn before_cleanup(
        &mut self,
        recovery_allowed: bool,
        connection_admission: Option<std::sync::Weak<tokio::sync::Semaphore>>,
    ) {
        self.entered
            .take()
            .expect("single observation")
            .send(Observation {
                worker: std::thread::current().id(),
                recovery_allowed,
                connection_admission,
            })
            .unwrap_or_else(|_| panic!("cleanup observation receiver"));
        self.resume
            .recv_timeout(Duration::from_secs(10))
            .expect("resume cleanup worker");
    }

    pub(in crate::pgwire::connection) fn record_call(&self) {
        self.calls.fetch_add(1, Ordering::Release);
    }

    pub(in crate::pgwire::connection) fn finish(self) {
        let _ = self.completed.send(());
    }
}
