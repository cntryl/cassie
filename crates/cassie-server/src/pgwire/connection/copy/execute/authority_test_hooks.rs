//! Pause after the real COPY owner check, before predicate evaluation.

use crate::app::{Cassie, CassieSession, FailureAuthority};
use std::cell::RefCell;
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

struct Hook {
    id: Uuid,
    runtime: usize,
    session: FailureAuthority,
    entered: SyncSender<()>,
    resume: Receiver<()>,
}

thread_local! {
    static NEXT: RefCell<Option<Hook>> = const { RefCell::new(None) };
}

pub(in crate::pgwire::connection::copy) struct Guard(Uuid);

impl Drop for Guard {
    fn drop(&mut self) {
        NEXT.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.as_ref().is_some_and(|hook| hook.id == self.0) {
                *slot = None;
            }
        });
    }
}

pub(in crate::pgwire::connection::copy) fn install_after_owner_check(
    cassie: &Cassie,
    session: &CassieSession,
    entered: SyncSender<()>,
    resume: Receiver<()>,
) -> Guard {
    let id = Uuid::new_v4();
    let hook = Hook {
        id,
        runtime: Arc::as_ptr(cassie.runtime()) as usize,
        session: FailureAuthority::capture(session),
        entered,
        resume,
    };
    NEXT.with(|slot| assert!(slot.replace(Some(hook)).is_none(), "nested COPY owner hook"));
    Guard(id)
}

pub(super) fn after_owner_check(cassie: &Cassie, session: &CassieSession) {
    let hook = NEXT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_some_and(|hook| {
            hook.runtime == Arc::as_ptr(cassie.runtime()) as usize
                && hook.session.captures_session_for_test(session)
        }) {
            slot.take()
        } else {
            None
        }
    });
    if let Some(hook) = hook {
        hook.entered.send(()).expect("COPY owner observation");
        hook.resume
            .recv_timeout(Duration::from_secs(5))
            .expect("resume COPY predicate");
    }
}
