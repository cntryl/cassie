//! Synchronous observation of admitted COPY memory immediately before ingest.

use crate::runtime::QueryExecutionControls;
use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, SyncSender};

thread_local! {
    static NEXT: RefCell<Option<SyncSender<usize>>> = const { RefCell::new(None) };
}

pub struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        NEXT.with(|slot| *slot.borrow_mut() = None);
    }
}

pub fn observe_before_ingest() -> (Guard, Receiver<usize>) {
    let (sender, receiver) = mpsc::sync_channel(1);
    NEXT.with(|slot| {
        assert!(
            slot.replace(Some(sender)).is_none(),
            "nested COPY memory observer"
        );
    });
    (Guard, receiver)
}

pub(super) fn before_ingest(controls: &QueryExecutionControls) {
    NEXT.with(|slot| {
        if let Some(sender) = slot.borrow_mut().take() {
            sender
                .send(controls.current_query_memory_bytes())
                .expect("COPY memory observation");
        }
    });
}
