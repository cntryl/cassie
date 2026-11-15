//! Capture-wait and post-admission ownership probes for pgwire workers.

use super::*;
use crate::config::CassieRuntimeConfig;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

fn assert_capture_interruption(deadline: bool) {
    let mut config = CassieRuntimeConfig::default();
    config.limits.max_query_workers = 1;
    config.limits.query_timeout_ms = 0;
    let cassie = Cassie::new_in_memory_with_config(config).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    session.begin_transaction(None).expect("begin");
    let owner = session.transaction_unique_owner_id();
    let (held_tx, held_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let held = Arc::new(AtomicBool::new(false));
    let holder_flag = Arc::clone(&held);
    let holding_session = session.clone();
    let holder = std::thread::spawn(move || {
        holding_session.hold_transaction_for_test(&held_tx, &release_rx, holder_flag);
    });
    held_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("root mutex held");
    let (contended_tx, contended_rx) = mpsc::sync_channel(1);
    let cancellation = QueryCancellationHandle::new();
    let worker_cancellation = cancellation.clone();
    let old_cassie = cassie.clone();
    let old_session = session.clone();
    let worker = std::thread::spawn(move || {
        // The observer fires on the capture's actual failed try-lock. In the
        // deadline case it expires the same controls instance used by SQL.
        let _hook = super::authority_test_hooks::install_capture_interruption(
            &old_cassie,
            &old_session,
            contended_tx,
            deadline,
        );
        old_cassie.execute_pgwire_sql_with_cancellation(
            &old_session,
            ("SELECT 1", "SELECT 1"),
            &worker_cancellation,
        )
    });
    contended_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("actual capture contention");
    let held_during = held.load(Ordering::Acquire);
    if !deadline {
        cancellation.cancel();
    }
    release_tx.send(()).expect("release root mutex");
    holder.join().expect("holder");
    let result = worker.join().expect("worker");
    let current = session.transaction_unique_owner_id();
    let failed = session.is_transaction_failed();
    let permit = cassie.runtime.try_begin_running_query();
    let permit_released = permit.is_some();
    drop(permit);
    session.rollback_transaction();
    assert!(held_during);
    if deadline {
        assert!(matches!(result, Err(CassieError::DeadlineExceeded)));
    } else {
        assert!(matches!(result, Err(CassieError::QueryCanceled)));
    }
    assert_eq!(current, owner);
    assert!(failed);
    assert!(permit_released);
}

#[test]
fn should_fail_current_owner_when_pgwire_capture_wait_is_canceled() {
    const CASE: &str = "pgwire_capture_canceled";
    if !crate::app::query_failure_watchdog::enter_child("app::query_prepared::authority_capture_tests::should_fail_current_owner_when_pgwire_capture_wait_is_canceled", CASE) { return; }
    // Arrange: the helper holds the root mutex and awaits real capture contention.
    // Act: it cancels and releases the lock.
    // Assert: the helper checks QueryCanceled, the failed owner and released slot.
    assert_capture_interruption(false);
    crate::app::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_fail_current_owner_when_pgwire_capture_wait_exceeds_deadline() {
    const CASE: &str = "pgwire_capture_deadline";
    if !crate::app::query_failure_watchdog::enter_child("app::query_prepared::authority_capture_tests::should_fail_current_owner_when_pgwire_capture_wait_exceeds_deadline", CASE) { return; }
    // Arrange: the helper holds the root mutex and awaits real capture contention.
    // Act: the observer expires the worker's own controls and the helper releases the lock.
    // Assert: the helper checks DeadlineExceeded, the failed owner and released slot.
    assert_capture_interruption(true);
    crate::app::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_capture_replacement_after_pgwire_worker_admission() {
    const CASE: &str = "pgwire_capture_replacement";
    if !crate::app::query_failure_watchdog::enter_child("app::query_prepared::authority_capture_tests::should_capture_replacement_after_pgwire_worker_admission", CASE) { return; }
    {
        // Arrange: pause after the actual worker permit and timeout snapshot,
        // before capture, without retaining the transaction mutex.
        let mut config = CassieRuntimeConfig::default();
        config.limits.max_query_workers = 1;
        let cassie = Cassie::new_in_memory_with_config(config).expect("memory fixture");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        session.begin_transaction(None).expect("begin A");
        let owner_a = session.transaction_unique_owner_id();
        let (admitted_tx, admitted_rx) = mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        let (captured_tx, captured_rx) = mpsc::sync_channel(1);
        let old_cassie = cassie.clone();
        let old_session = session.clone();
        let worker = std::thread::spawn(move || {
            let _hook = super::authority_test_hooks::install_after_admission(
                &old_cassie,
                &old_session,
                admitted_tx,
                resume_rx,
            );
            old_cassie.execute_pgwire_query(
                &old_session,
                &QueryCancellationHandle::new(),
                |controls, authority| {
                    captured_tx
                        .send(authority.expect("admitted authority").owner_for_test())
                        .expect("actual captured owner");
                    let parsed = crate::sql::parse_statement("SELECT 1").expect("parse");
                    let fingerprint = crate::runtime::sql_fingerprint(&parsed);
                    old_cassie.execute_pgwire_statement_core_for_operation(
                        &old_session,
                        parsed,
                        fingerprint,
                        (Vec::new(), &[]),
                        ExecutionMode::SimpleQuery,
                        (controls, authority),
                    )
                },
            )
        });
        admitted_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("actual admission");
        session.rollback_transaction();
        session.begin_transaction(None).expect("begin B");
        let owner_b = session.transaction_unique_owner_id();
        let epoch_b = session.rollback_epoch_for_test();

        // Act
        resume_tx.send(()).expect("resume capture");
        let captured = captured_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("captured owner");
        let result = worker.join().expect("worker");
        let current = session.transaction_unique_owner_id();
        let healthy = session.is_transaction_active() && !session.is_transaction_failed();
        session.rollback_transaction();

        // Assert: this establishes capture linearization, not API-entry ownership.
        assert_ne!(owner_a, owner_b);
        assert_eq!(captured, Some((owner_b, epoch_b)));
        assert_eq!(current, owner_b);
        assert!(healthy);
        assert!(result.is_ok());
    }
    crate::app::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_preserve_pgwire_unsupported_error_before_cancellation() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    session.begin_transaction(None).expect("begin");
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();

    // Act
    let result = cassie.execute_pgwire_sql_with_cancellation(
        &session,
        ("LISTEN channel", "LISTEN channel"),
        &cancellation,
    );
    let failed = session.is_transaction_failed();
    session.rollback_transaction();

    // Assert
    assert!(
        matches!(result, Err(CassieError::Unsupported(ref message)) if message == "LISTEN is not supported")
    );
    assert!(failed);
}
