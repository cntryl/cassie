//! Scoped observations at real query admission and authority capture boundaries.

use crate::app::{Cassie, CassieSession, FailureAuthority};
use crate::runtime::QueryExecutionControls;
use std::cell::RefCell;
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

type Captured = Option<(Uuid, u64)>;

enum Action {
    AfterCapture {
        captured: SyncSender<Captured>,
        resume: Receiver<()>,
    },
    AfterHealthy {
        checked: SyncSender<()>,
        resume: Receiver<()>,
    },
    Contention {
        contended: SyncSender<()>,
        expire_deadline: bool,
    },
    AfterAdmission {
        admitted: SyncSender<()>,
        resume: Receiver<()>,
        captured: SyncSender<Captured>,
    },
    CaptureOnly {
        captured: SyncSender<Captured>,
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
    NEXT.with(|slot| assert!(slot.replace(Some(hook)).is_none(), "nested authority hook"));
    Guard(id)
}

pub(crate) fn install_after_root_capture(
    cassie: &Cassie,
    session: &CassieSession,
    captured: SyncSender<Captured>,
    resume: Receiver<()>,
) -> Guard {
    install(cassie, session, Action::AfterCapture { captured, resume })
}

pub(crate) fn install_after_healthy_recovery_check(
    cassie: &Cassie,
    session: &CassieSession,
    checked: SyncSender<()>,
    resume: Receiver<()>,
) -> Guard {
    install(cassie, session, Action::AfterHealthy { checked, resume })
}

pub(crate) fn install_on_capture_contention(
    cassie: &Cassie,
    session: &CassieSession,
    contended: SyncSender<()>,
    expire_deadline: bool,
) -> Guard {
    install(
        cassie,
        session,
        Action::Contention {
            contended,
            expire_deadline,
        },
    )
}

pub(crate) fn install_after_admission_before_capture(
    cassie: &Cassie,
    session: &CassieSession,
    admitted: SyncSender<()>,
    resume: Receiver<()>,
    captured: SyncSender<Captured>,
) -> Guard {
    install(
        cassie,
        session,
        Action::AfterAdmission {
            admitted,
            resume,
            captured,
        },
    )
}

fn matching(hook: &Hook, cassie: &Cassie, session: &CassieSession) -> bool {
    hook.runtime == Arc::as_ptr(&cassie.runtime) as usize
        && hook.session.captures_session_for_test(session)
}

pub(super) fn after_healthy_recovery_check(cassie: &Cassie, session: &CassieSession) {
    let hook = NEXT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_some_and(|hook| {
            matching(hook, cassie, session) && matches!(&hook.action, Action::AfterHealthy { .. })
        }) {
            slot.take()
        } else {
            None
        }
    });
    if let Some(Hook {
        action: Action::AfterHealthy { checked, resume },
        ..
    }) = hook
    {
        checked.send(()).expect("healthy recovery observation");
        resume
            .recv_timeout(Duration::from_secs(5))
            .expect("resume admission");
    }
}

pub(super) fn after_admission_before_capture(cassie: &Cassie, session: &CassieSession) {
    let hook = NEXT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_some_and(|hook| {
            matching(hook, cassie, session) && matches!(&hook.action, Action::AfterAdmission { .. })
        }) {
            slot.take()
        } else {
            None
        }
    });
    if let Some(Hook {
        id,
        runtime,
        session: identity,
        action,
    }) = hook
    {
        match action {
            Action::AfterAdmission {
                admitted,
                resume,
                captured,
            } => {
                admitted.send(()).expect("query admission");
                resume
                    .recv_timeout(Duration::from_secs(5))
                    .expect("resume capture");
                NEXT.with(|slot| {
                    assert!(slot
                        .replace(Some(Hook {
                            id,
                            runtime,
                            session: identity,
                            action: Action::CaptureOnly { captured }
                        }))
                        .is_none());
                });
            }
            _ => unreachable!("filtered action"),
        }
    }
}

pub(super) fn on_capture_contended(
    cassie: &Cassie,
    session: &CassieSession,
    controls: &mut QueryExecutionControls,
) {
    let hook = NEXT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_some_and(|hook| {
            matching(hook, cassie, session) && matches!(&hook.action, Action::Contention { .. })
        }) {
            slot.take()
        } else {
            None
        }
    });
    if let Some(Hook {
        action: Action::Contention {
            contended,
            expire_deadline,
        },
        ..
    }) = hook
    {
        if expire_deadline {
            controls.deadline = Some(Instant::now());
        }
        contended.send(()).expect("capture contention");
    }
}

pub(super) fn after_root_capture(
    cassie: &Cassie,
    session: &CassieSession,
    authority: &FailureAuthority,
) {
    let hook = NEXT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_some_and(|hook| {
            matching(hook, cassie, session)
                && matches!(
                    &hook.action,
                    Action::AfterCapture { .. } | Action::CaptureOnly { .. }
                )
        }) {
            slot.take()
        } else {
            None
        }
    });
    if let Some(hook) = hook {
        match hook.action {
            Action::AfterCapture { captured, resume } => {
                captured
                    .send(authority.owner_for_test())
                    .expect("authority observation");
                resume
                    .recv_timeout(Duration::from_secs(5))
                    .expect("resume authority");
            }
            Action::CaptureOnly { captured } => {
                captured
                    .send(authority.owner_for_test())
                    .expect("authority observation");
            }
            _ => unreachable!("filtered action"),
        }
    }
}
