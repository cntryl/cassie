//! One-shot observers for the refresh executed on the installing worker thread.

use std::cell::Cell;
use std::sync::{mpsc, Arc, Barrier};

type Pause = (Arc<Barrier>, Arc<Barrier>);

thread_local! {
    static BEFORE_REPLACE: Cell<Option<Pause>> = const { Cell::new(None) };
    static AFTER_DROP: Cell<Option<Pause>> = const { Cell::new(None) };
    static GATE_WAIT: Cell<Option<mpsc::Sender<()>>> = const { Cell::new(None) };
}

#[doc(hidden)]
pub fn set_materialized_projection_replace_start_barriers(
    ready: Option<Arc<Barrier>>,
    resume: Option<Arc<Barrier>>,
) {
    BEFORE_REPLACE.set(ready.zip(resume));
}

#[doc(hidden)]
pub fn set_materialized_projection_replace_barriers(
    dropped: Option<Arc<Barrier>>,
    resume: Option<Arc<Barrier>>,
) {
    AFTER_DROP.set(dropped.zip(resume));
}

#[doc(hidden)]
pub fn set_materialized_projection_refresh_wait_observer(sender: mpsc::Sender<()>) {
    GATE_WAIT.set(Some(sender));
}

pub(super) fn observe_refresh_gate_wait() {
    if let Some(sender) = GATE_WAIT.take() {
        let _ = sender.send(());
    }
}

pub(super) fn pause_before_materialized_projection_replace() {
    pause(BEFORE_REPLACE.take());
}

pub(super) fn pause_after_materialized_projection_drop() {
    pause(AFTER_DROP.take());
}

fn pause(barriers: Option<Pause>) {
    if let Some((ready, resume)) = barriers {
        ready.wait();
        resume.wait();
    }
}
