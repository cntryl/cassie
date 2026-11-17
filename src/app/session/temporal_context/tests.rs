//! Clock publication follows the actual root, including detached statement lineage.
use super::*;
use crate::app::{Cassie, FailureAuthority};
use crate::config::CassieRuntimeConfig;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::Duration;

thread_local! {
    static BEFORE: RefCell<Option<(SyncSender<()>, Receiver<()>)>> = const { RefCell::new(None) };
    static ROOT_OBSERVER: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

pub(super) fn before_clock_owner() {
    if let Some((entered, resume)) = BEFORE.with(|slot| slot.borrow_mut().take()) {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(10)).unwrap();
    }
}

pub(super) fn observe_clock_publication(session: &CassieSession) {
    if let Some(observed) = ROOT_OBSERVER.with(|slot| slot.borrow_mut().take()) {
        let root = session.operation_failure_authority().map_or_else(
            || Arc::clone(&session.transaction),
            |authority| authority.root.upgrade().unwrap(),
        );
        assert!(
            root.try_lock().is_none(),
            "actual root must remain held through clock publication"
        );
        assert!(
            session.transaction_clock.try_lock().is_none(),
            "clock is held after root"
        );
        observed.store(true, Ordering::Release);
    }
}

fn fixture() -> (Cassie, CassieSession) {
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("clock_owner", None);
    session.begin_transaction(None).unwrap();
    (cassie, session)
}

#[test]
fn should_refuse_an_old_root_or_fork_clock_without_resetting_b_now() {
    for detached in [false, true] {
        // Arrange
        let (cassie, session) = fixture();
        let worker_session = if detached {
            session.fork_statement_batch().unwrap().session().clone()
        } else {
            session.clone()
        };
        let worker_cassie = cassie.clone();
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            BEFORE.with(|slot| assert!(slot.replace(Some((entered_tx, resume_rx))).is_none()));
            worker_cassie.execute_sql(&worker_session, "SELECT now()", vec![])
        });
        entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();

        // Act
        session.rollback_transaction();
        session.begin_transaction(None).unwrap();
        let first = cassie
            .execute_sql(&session, "SELECT now()", vec![])
            .unwrap();
        let clock = *session.transaction_clock.lock();
        let owner = session.transaction_unique_owner_id();
        resume_tx.send(()).unwrap();
        let stale = worker.join().unwrap();
        let retained = *session.transaction_clock.lock();
        let second = cassie
            .execute_sql(&session, "SELECT now()", vec![])
            .unwrap();

        // Assert
        assert_eq!(stale.unwrap_err().sql_state(), "25000");
        assert_eq!(retained, clock);
        assert_eq!(first.rows(), second.rows());
        assert_eq!(session.transaction_unique_owner_id(), owner);
        assert!(!session.is_transaction_failed());
    }
}

#[test]
fn should_install_the_first_current_clock_under_root_then_clock_locks() {
    // Arrange
    let (cassie, session) = fixture();
    let operation = session.with_new_wire_operation();
    FailureAuthority::capture_for_operation(&operation);
    let observed = Arc::new(AtomicBool::new(false));
    ROOT_OBSERVER.with(|slot| assert!(slot.replace(Some(Arc::clone(&observed))).is_none()));
    let controls = cassie.runtime.query_controls(std::time::Instant::now());

    // Act
    operation.install_temporal_session(&controls).unwrap();
    let temporal = controls.temporal_session().unwrap();
    let clock = *session.transaction_clock.lock();
    let batch = operation.fork_statement_batch().unwrap();
    let nested_controls = cassie.runtime.query_controls(std::time::Instant::now());
    batch
        .session()
        .install_temporal_session(&nested_controls)
        .unwrap();

    // Assert
    assert!(observed.load(Ordering::Acquire));
    assert_eq!(
        clock,
        Some((session.transaction_generation(), temporal.transaction_start))
    );
    assert_eq!(
        nested_controls
            .temporal_session()
            .unwrap()
            .transaction_start,
        temporal.transaction_start
    );
}

#[test]
fn should_keep_idle_statement_start_separate_from_the_shared_transaction_clock() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(&session, "SELECT now()", vec![])
        .unwrap();
    session.rollback_transaction();
    let prior = *session.transaction_clock.lock();
    let controls = cassie.runtime.query_controls(std::time::Instant::now());

    // Act
    session
        .with_statement_text("SELECT now()", &controls)
        .unwrap();
    let temporal = controls.temporal_session().unwrap();

    // Assert
    assert_eq!(temporal.transaction_start, temporal.statement_start);
    assert_eq!(*session.transaction_clock.lock(), prior);
}

#[test]
fn should_allow_only_the_own_retired_root_to_install_an_idle_statement_clock() {
    // Arrange
    let (cassie, session) = fixture();
    let operation = session.with_new_wire_operation();
    FailureAuthority::capture_for_operation(&operation);
    let controls = cassie.runtime.query_controls(std::time::Instant::now());
    operation.install_temporal_session(&controls).unwrap();
    let fork = operation.fork_statement_batch().unwrap();
    let prior = *session.transaction_clock.lock();
    cassie.commit_transaction(&operation).unwrap();
    let idle = cassie.runtime.query_controls(std::time::Instant::now());

    // Act
    let current = operation.with_statement_text("SELECT now()", &idle);
    let fork_idle = fork
        .session()
        .with_statement_text("SELECT now()", &controls);
    session.begin_transaction(None).unwrap();
    session.rollback_transaction();
    let aba = operation.with_statement_text("SELECT now()", &controls);

    // Assert
    current.unwrap();
    let temporal = idle.temporal_session().unwrap();
    assert_eq!(temporal.transaction_start, temporal.statement_start);
    assert_eq!(fork_idle.unwrap_err().sql_state(), "25000");
    assert_eq!(aba.unwrap_err().sql_state(), "25000");
    assert_eq!(*session.transaction_clock.lock(), prior);
}

#[test]
fn should_not_adopt_another_actors_commit_as_an_idle_clock_receipt() {
    // Arrange
    let (cassie, session) = fixture();
    let operation = session.with_new_wire_operation();
    FailureAuthority::capture_for_operation(&operation);
    cassie.commit_transaction(&session).unwrap();
    let controls = cassie.runtime.query_controls(std::time::Instant::now());

    // Act
    let result = operation.with_statement_text("SELECT now()", &controls);

    // Assert
    assert_eq!(result.unwrap_err().sql_state(), "25000");
    assert!(session.transaction_clock.lock().is_none());
}

#[test]
fn should_preserve_now_through_an_actual_nested_procedure_call() {
    // Arrange
    let (cassie, session) = fixture();
    session.rollback_transaction();
    cassie
        .execute_sql(
            &session,
            r#"CREATE PROCEDURE nested_clock() AS "SELECT now()""#,
            vec![],
        )
        .unwrap();
    session.begin_transaction(None).unwrap();
    let first = cassie
        .execute_sql(&session, "SELECT now()", vec![])
        .unwrap();
    let before = *session.transaction_clock.lock();

    // Act
    cassie
        .execute_sql(&session, "CALL nested_clock()", vec![])
        .unwrap();
    let second = cassie
        .execute_sql(&session, "SELECT now()", vec![])
        .unwrap();

    // Assert
    assert_eq!(first.rows(), second.rows());
    assert_eq!(*session.transaction_clock.lock(), before);
    assert!(!session.is_transaction_failed());
}
