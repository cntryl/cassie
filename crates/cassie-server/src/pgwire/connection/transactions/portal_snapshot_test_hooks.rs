//! Test-only observations after a real transaction-status snapshot.

use crate::app::{CassieSession, FailureAuthority, SessionTransactionStatus};
use std::cell::RefCell;
use std::sync::mpsc::{Receiver, SyncSender};
use std::time::Duration;
use uuid::Uuid;

struct Hook {
    id: Uuid,
    session: FailureAuthority,
    observed: SyncSender<SessionTransactionStatus>,
    resume: Receiver<()>,
}

thread_local! {
    static NEXT: RefCell<Option<Hook>> = const { RefCell::new(None) };
}

pub(crate) struct Guard(Uuid);

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

pub(crate) fn install(
    session: &CassieSession,
    observed: SyncSender<SessionTransactionStatus>,
    resume: Receiver<()>,
) -> Guard {
    let id = Uuid::new_v4();
    let hook = Hook {
        id,
        session: FailureAuthority::identity_only_for_test(session),
        observed,
        resume,
    };
    NEXT.with(|slot| {
        assert!(
            slot.replace(Some(hook)).is_none(),
            "nested portal snapshot hook"
        );
    });
    Guard(id)
}

pub(super) fn after_snapshot(session: &CassieSession, status: SessionTransactionStatus) {
    let hook = NEXT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot
            .as_ref()
            .is_some_and(|hook| hook.session.captures_session_for_test(session))
        {
            slot.take()
        } else {
            None
        }
    });
    if let Some(hook) = hook {
        hook.observed.send(status).expect("portal cleanup snapshot");
        hook.resume
            .recv_timeout(Duration::from_secs(5))
            .expect("resume portal cleanup");
    }
}
