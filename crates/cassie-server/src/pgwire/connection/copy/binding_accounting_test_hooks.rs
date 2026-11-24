//! Scoped admission probes; compiled only for the COPY test module.

use crate::app::{Cassie, CassieSession};
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls, QueryMemoryReservation};
use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

pub(super) enum Remaining {
    Exact,
    OneByteShort,
}

enum Action {
    Pressure(Remaining),
    Cancel(QueryCancellationHandle),
    Deadline,
}

pub(super) struct Observation {
    pub(super) controls: QueryExecutionControls,
    pub(super) charge: usize,
    pub(super) remaining: usize,
    pub(super) owner_present: bool,
    pub(super) validated: bool,
    pub(super) charge_attempted: bool,
}

struct Hook {
    id: Uuid,
    runtime: usize,
    owner: Uuid,
    action: Action,
    observed: SyncSender<Observation>,
}

thread_local! {
    static NEXT: RefCell<Option<Hook>> = const { RefCell::new(None) };
}

pub(super) struct Guard(Uuid);

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

fn install(
    cassie: &Cassie,
    session: &CassieSession,
    action: Action,
) -> (Guard, Receiver<Observation>) {
    let (sender, receiver) = mpsc::sync_channel(1);
    let id = Uuid::new_v4();
    NEXT.with(|slot| {
        let old = slot.replace(Some(Hook {
            id,
            runtime: Arc::as_ptr(cassie.runtime()) as usize,
            owner: session.transaction_unique_owner_id(),
            action,
            observed: sender,
        }));
        assert!(old.is_none(), "nested COPY accounting hook");
    });
    (Guard(id), receiver)
}

pub(super) fn leave_before_charge(
    cassie: &Cassie,
    session: &CassieSession,
    remaining: Remaining,
) -> (Guard, Receiver<Observation>) {
    install(cassie, session, Action::Pressure(remaining))
}

pub(super) fn cancel_after_validation(
    cassie: &Cassie,
    session: &CassieSession,
    cancellation: QueryCancellationHandle,
) -> (Guard, Receiver<Observation>) {
    install(cassie, session, Action::Cancel(cancellation))
}

pub(super) fn expire_after_validation(
    cassie: &Cassie,
    session: &CassieSession,
) -> (Guard, Receiver<Observation>) {
    install(cassie, session, Action::Deadline)
}

pub(super) struct Pending {
    observation: Observation,
    observed: SyncSender<Observation>,
    _filler: Option<QueryMemoryReservation>,
}

pub(super) fn before_admission(
    cassie: &Cassie,
    session: &CassieSession,
    controls: &mut QueryExecutionControls,
    charge: usize,
    owner_present: bool,
) -> Option<Pending> {
    let hook = NEXT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_some_and(|hook| {
            hook.runtime == Arc::as_ptr(cassie.runtime()) as usize
                && hook.owner == session.transaction_unique_owner_id()
        }) {
            slot.take()
        } else {
            None
        }
    })?;
    assert!(charge > 0, "COPY binding charge must be positive");
    let used = controls.current_query_memory_bytes();
    let capacity = controls
        .query_memory_budget_bytes
        .checked_sub(used)
        .expect("query budget");
    let mut remaining = capacity;
    let filler = match hook.action {
        Action::Pressure(target) => {
            remaining = match target {
                Remaining::Exact => charge,
                Remaining::OneByteShort => charge.checked_sub(1).expect("positive charge"),
            };
            let filler = capacity
                .checked_sub(remaining)
                .expect("prior peak admitted");
            Some(
                controls
                    .reserve_query_memory(filler)
                    .expect("same-tracker filler"),
            )
        }
        Action::Cancel(cancellation) => {
            cancellation.cancel();
            None
        }
        Action::Deadline => {
            controls.deadline = Some(Instant::now());
            None
        }
    };
    Some(Pending {
        observation: Observation {
            controls: controls.clone(),
            charge,
            remaining,
            owner_present,
            validated: true,
            charge_attempted: false,
        },
        observed: hook.observed,
        _filler: filler,
    })
}

impl Pending {
    pub(super) fn finish(mut self, charge_attempted: bool) {
        self.observation.charge_attempted = charge_attempted;
        self.observed
            .send(self.observation)
            .expect("accounting observation");
    }
}
