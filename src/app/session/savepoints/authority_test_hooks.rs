//! Scoped observations at real savepoint failure boundaries.

use super::super::{CassieError, CassieSession, FailureAuthority};
use crate::app::TransactionErrorKind;
use crate::runtime::QueryCancellationHandle;
use std::cell::RefCell;
use std::sync::mpsc::{Receiver, SyncSender};
use std::time::Duration;
use uuid::Uuid;

enum Action {
    CancelPrune {
        cancellation: QueryCancellationHandle,
        observed: SyncSender<u64>,
    },
    PauseRestoreError {
        observed: SyncSender<(Uuid, u64, TransactionErrorKind)>,
        resume: Receiver<()>,
    },
}

struct Hook {
    id: Uuid,
    session: FailureAuthority,
    action: Action,
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

fn install(session: &CassieSession, action: Action) -> Guard {
    let id = Uuid::new_v4();
    let hook = Hook {
        id,
        session: FailureAuthority::capture(session),
        action,
    };
    NEXT.with(|slot| assert!(slot.replace(Some(hook)).is_none(), "nested savepoint hook"));
    Guard(id)
}

impl CassieSession {
    pub(crate) fn cancel_savepoint_prune_for_test(
        &self,
        cancellation: QueryCancellationHandle,
        observed: SyncSender<u64>,
    ) -> Guard {
        install(
            self,
            Action::CancelPrune {
                cancellation,
                observed,
            },
        )
    }

    pub(crate) fn observe_savepoint_restore_error_for_test(
        &self,
        observed: SyncSender<(Uuid, u64, TransactionErrorKind)>,
        resume: Receiver<()>,
    ) -> Guard {
        install(self, Action::PauseRestoreError { observed, resume })
    }
}

pub(super) fn before_prune(session: &CassieSession, epoch: u64) {
    let hook = NEXT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_some_and(|hook| {
            hook.session.captures_session_for_test(session)
                && matches!(&hook.action, Action::CancelPrune { .. })
        }) {
            slot.take()
        } else {
            None
        }
    });
    if let Some(Hook {
        action: Action::CancelPrune {
            cancellation,
            observed,
        },
        ..
    }) = hook
    {
        observed.try_send(epoch).expect("prune observation");
        cancellation.cancel();
    }
}

pub(super) fn after_restore_error(
    session: &CassieSession,
    owner: Uuid,
    epoch: u64,
    error: &CassieError,
) {
    let hook = NEXT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_some_and(|hook| {
            hook.session.captures_session_for_test(session)
                && matches!(&hook.action, Action::PauseRestoreError { .. })
        }) {
            slot.take()
        } else {
            None
        }
    });
    if let Some(Hook {
        action: Action::PauseRestoreError { observed, resume },
        ..
    }) = hook
    {
        let CassieError::Transaction { kind, .. } = error else {
            panic!("expected transaction restore error")
        };
        observed
            .send((owner, epoch, *kind))
            .expect("restore observation");
        resume
            .recv_timeout(Duration::from_secs(5))
            .expect("resume restore error");
    }
}
