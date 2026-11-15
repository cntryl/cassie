//! Scoped, worker-local observations at actual pgwire authority boundaries.

use crate::app::{Cassie, CassieError, CassieSession, FailureAuthority, QueryResult};
use crate::runtime::QueryExecutionControls;
use std::cell::RefCell;
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

type Captured = Option<(Uuid, u64)>;

enum Action {
    WorkerError {
        observed: SyncSender<Captured>,
        resume: Receiver<()>,
    },
    WorkerResult {
        observed: SyncSender<(Captured, bool)>,
    },
    Contention {
        observed: SyncSender<()>,
        expire_deadline: bool,
    },
    AfterAdmission {
        observed: SyncSender<()>,
        resume: Receiver<()>,
    },
    Healthy {
        observed: SyncSender<()>,
        resume: Receiver<()>,
    },
    StatementText {
        observed: SyncSender<()>,
        resume: Receiver<()>,
    },
    ImplicitDdl {
        observed: SyncSender<Captured>,
    },
    Nested {
        observed: SyncSender<(bool, Captured)>,
    },
}

struct Hook {
    id: Uuid,
    runtime: usize,
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

fn install(cassie: &Cassie, session: &CassieSession, action: Action) -> Guard {
    let id = Uuid::new_v4();
    let hook = Hook {
        id,
        runtime: Arc::as_ptr(&cassie.runtime) as usize,
        session: FailureAuthority::identity_only_for_test(session),
        action,
    };
    NEXT.with(|slot| {
        assert!(
            slot.replace(Some(hook)).is_none(),
            "nested pgwire authority hook"
        );
    });
    Guard(id)
}

pub(crate) fn install_after_worker_error(
    cassie: &Cassie,
    session: &CassieSession,
    observed: SyncSender<Captured>,
    resume: Receiver<()>,
) -> Guard {
    install(cassie, session, Action::WorkerError { observed, resume })
}

pub(crate) fn install_after_worker_result(
    cassie: &Cassie,
    session: &CassieSession,
    observed: SyncSender<(Captured, bool)>,
) -> Guard {
    install(cassie, session, Action::WorkerResult { observed })
}

pub(crate) fn install_on_capture_contention(
    cassie: &Cassie,
    session: &CassieSession,
    observed: SyncSender<()>,
) -> Guard {
    install_capture_interruption(cassie, session, observed, false)
}

pub(crate) fn install_capture_interruption(
    cassie: &Cassie,
    session: &CassieSession,
    observed: SyncSender<()>,
    expire_deadline: bool,
) -> Guard {
    install(
        cassie,
        session,
        Action::Contention {
            observed,
            expire_deadline,
        },
    )
}

pub(crate) fn install_after_admission(
    cassie: &Cassie,
    session: &CassieSession,
    observed: SyncSender<()>,
    resume: Receiver<()>,
) -> Guard {
    install(cassie, session, Action::AfterAdmission { observed, resume })
}

pub(crate) fn install_after_healthy_check(
    cassie: &Cassie,
    session: &CassieSession,
    observed: SyncSender<()>,
    resume: Receiver<()>,
) -> Guard {
    install(cassie, session, Action::Healthy { observed, resume })
}

pub(crate) fn install_on_implicit_ddl(
    cassie: &Cassie,
    session: &CassieSession,
    observed: SyncSender<Captured>,
) -> Guard {
    install(cassie, session, Action::ImplicitDdl { observed })
}

pub(crate) fn install_on_nested(
    cassie: &Cassie,
    session: &CassieSession,
    observed: SyncSender<(bool, Captured)>,
) -> Guard {
    install(cassie, session, Action::Nested { observed })
}

pub(crate) fn install_after_statement_text(
    cassie: &Cassie,
    session: &CassieSession,
    observed: SyncSender<()>,
    resume: Receiver<()>,
) -> Guard {
    install(cassie, session, Action::StatementText { observed, resume })
}

fn take_if(
    cassie: &Cassie,
    session: &CassieSession,
    predicate: impl FnOnce(&Action) -> bool,
) -> Option<Hook> {
    NEXT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_some_and(|hook| {
            hook.runtime == Arc::as_ptr(&cassie.runtime) as usize
                && hook.session.captures_session_for_test(session)
                && predicate(&hook.action)
        }) {
            slot.take()
        } else {
            None
        }
    })
}

pub(super) fn after_worker_result(
    cassie: &Cassie,
    session: &CassieSession,
    authority: Option<&FailureAuthority>,
    result: &Result<QueryResult, CassieError>,
) {
    let hook = take_if(cassie, session, |action| {
        matches!(action, Action::WorkerResult { .. })
            || result.is_err() && matches!(action, Action::WorkerError { .. })
    });
    if let Some(hook) = hook {
        let captured = authority.and_then(FailureAuthority::owner_for_test);
        match hook.action {
            Action::WorkerError { observed, resume } => {
                observed.send(captured).expect("actual worker error");
                resume
                    .recv_timeout(Duration::from_secs(5))
                    .expect("resume worker completion");
            }
            Action::WorkerResult { observed } => observed
                .send((captured, result.is_ok()))
                .expect("actual worker result"),
            _ => unreachable!("filtered action"),
        }
    }
}

pub(super) fn on_capture_contended(
    cassie: &Cassie,
    session: &CassieSession,
    controls: &mut QueryExecutionControls,
) {
    if let Some(Hook {
        action: Action::Contention {
            observed,
            expire_deadline,
        },
        ..
    }) = take_if(cassie, session, |action| {
        matches!(action, Action::Contention { .. })
    }) {
        if expire_deadline {
            controls.deadline = Some(Instant::now());
        }
        observed.send(()).expect("actual capture contention");
    }
}

pub(super) fn after_admission(cassie: &Cassie, session: &CassieSession) {
    if let Some(Hook {
        action: Action::AfterAdmission { observed, resume },
        ..
    }) = take_if(cassie, session, |action| {
        matches!(action, Action::AfterAdmission { .. })
    }) {
        observed.send(()).expect("actual query admission");
        resume
            .recv_timeout(Duration::from_secs(5))
            .expect("resume capture");
    }
}

pub(super) fn after_healthy_check(cassie: &Cassie, session: &CassieSession) {
    if let Some(Hook {
        action: Action::Healthy { observed, resume },
        ..
    }) = take_if(cassie, session, |action| {
        matches!(action, Action::Healthy { .. })
    }) {
        observed.send(()).expect("actual healthy recovery check");
        resume
            .recv_timeout(Duration::from_secs(5))
            .expect("resume admission");
    }
}

pub(super) fn after_statement_text(cassie: &Cassie, session: &CassieSession) {
    if let Some(Hook {
        action: Action::StatementText { observed, resume },
        ..
    }) = take_if(cassie, session, |action| {
        matches!(action, Action::StatementText { .. })
    }) {
        observed
            .send(())
            .expect("actual statement-text installation");
        resume
            .recv_timeout(Duration::from_secs(5))
            .expect("resume statement execution");
    }
}

pub(crate) fn on_implicit_ddl(
    cassie: &Cassie,
    session: &CassieSession,
    authority: Option<&FailureAuthority>,
) {
    if let Some(Hook {
        action: Action::ImplicitDdl { observed },
        ..
    }) = take_if(cassie, session, |action| {
        matches!(action, Action::ImplicitDdl { .. })
    }) {
        observed
            .send(authority.and_then(FailureAuthority::owner_for_test))
            .expect("implicit DDL authority");
    }
}

pub(crate) fn on_nested(
    cassie: &Cassie,
    session: &CassieSession,
    authority: Option<&FailureAuthority>,
) {
    if let Some(Hook {
        action: Action::Nested { observed },
        ..
    }) = take_if(cassie, session, |action| {
        matches!(action, Action::Nested { .. })
    }) {
        observed
            .send((
                authority.is_some(),
                authority.and_then(FailureAuthority::owner_for_test),
            ))
            .expect("nested authority");
    }
}
