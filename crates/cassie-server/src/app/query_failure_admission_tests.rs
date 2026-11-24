//! Authored, unrun regression for query admission versus owner capture.

use super::{Cassie, CassieError};
use crate::config::CassieRuntimeConfig;
use crate::runtime::QueryCancellationHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

fn assert_saturated_request_finishes_before_transaction_unlock(active: bool, canceled: bool) {
    let mut config = CassieRuntimeConfig::default();
    config.limits.max_query_workers = 1;
    let cassie = Cassie::new_in_memory_with_config(config).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    if active {
        session.begin_transaction(None).expect("begin");
    }
    let occupied = cassie
        .runtime
        .try_begin_running_query()
        .expect("occupy only slot");
    let (held_tx, held_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let held = Arc::new(AtomicBool::new(false));
    let holder_flag = Arc::clone(&held);
    let holding_session = session.clone();
    let holder = std::thread::spawn(move || {
        // Future test-only seam takes the actual transaction mutex, signals
        // only once held, and waits for release/disconnect without a timeout.
        // The independent process watchdog bounds a faulty implementation.
        holding_session.hold_transaction_for_test(&held_tx, &release_rx, holder_flag);
    });
    held_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("transaction lock held");
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let requesting_cassie = cassie.clone();
    let requesting_session = session.clone();
    let cancellation = QueryCancellationHandle::new();
    if canceled {
        cancellation.cancel();
    }
    let request = std::thread::spawn(move || {
        let result = requesting_cassie.execute_sql_with_cancellation_converted(
            &requesting_session,
            "SELECT 1",
            vec![],
            &cancellation,
            |_, _| Ok(()),
        );
        result_tx.send(result).expect("request result");
    });

    // Act: the timeout guards liveness, not a latency requirement. Release the
    // mutex even on failure so both workers can be joined before asserting.
    let before_unlock = result_rx.recv_timeout(Duration::from_secs(5));
    let held_at_result = held.load(Ordering::Acquire);
    release_tx.send(()).expect("release transaction lock");
    holder.join().expect("holder");
    request.join().expect("request");
    drop(occupied);
    if active {
        session.rollback_transaction();
    }

    // Assert
    assert!(matches!(before_unlock,
        Ok(Err(CassieError::Execution(ref message))) if message == "query admission exhausted"));
    assert!(
        held_at_result,
        "transaction mutex must remain held until release"
    );
}

#[test]
fn should_reject_saturated_query_before_waiting_for_transaction_lock() {
    const CASE: &str = "saturated_owner_capture";
    if !super::query_failure_watchdog::enter_child(
        "app::query_failure_admission_tests::should_reject_saturated_query_before_waiting_for_transaction_lock",
        CASE,
    ) {
        return;
    }
    // Arrange: The watchdog bounds both transaction-state scenarios.
    // Act: Run each synchronized scenario.
    // Assert: The helper checks its result before returning.
    for active in [false, true] {
        assert_saturated_request_finishes_before_transaction_unlock(active, false);
    }
    super::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_preserve_saturated_admission_error_when_request_is_canceled() {
    const CASE: &str = "saturated_canceled_capture";
    if !super::query_failure_watchdog::enter_child(
        "app::query_failure_admission_tests::should_preserve_saturated_admission_error_when_request_is_canceled",
        CASE,
    ) {
        return;
    }
    // Arrange: The watchdog bounds this scenario.
    // Act: Run the synchronized scenario.
    // Assert: The helper checks its result before returning.
    assert_saturated_request_finishes_before_transaction_unlock(true, true);
    super::query_failure_watchdog::complete_child(CASE);
}

#[derive(Clone, Copy)]
enum CaptureCase {
    Cancellation,
    Deadline,
    CancellationBeforeDeadline,
    Unsupported,
    EmptyUser,
    MissingDatabase,
    Recovery,
}

fn assert_contended_capture_fails_its_current_owner(case: CaptureCase) {
    let mut config = CassieRuntimeConfig::default();
    config.limits.max_query_workers = 1;
    let cassie = Cassie::new_in_memory_with_config(config).expect("memory fixture");
    cassie.startup().expect("startup");
    let user = if matches!(case, CaptureCase::EmptyUser) {
        ""
    } else {
        "tester"
    };
    let database =
        matches!(case, CaptureCase::MissingDatabase).then(|| "missing_capture_db".to_owned());
    let session = cassie.create_session(user, database);
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
        .expect("transaction lock held");
    let (contended_tx, contended_rx) = mpsc::sync_channel(1);
    let old_cassie = cassie.clone();
    let old_session = session.clone();
    let cancellation = QueryCancellationHandle::new();
    let old_cancellation = cancellation.clone();
    let worker = std::thread::spawn(move || {
        // Future scoped hook signals only after actual post-admission try-lock
        // contention; deadline mode expires the actual request controls there.
        let _hook = super::query_lifecycle::authority_test_hooks::install_on_capture_contention(
            &old_cassie,
            &old_session,
            contended_tx,
            matches!(
                case,
                CaptureCase::Deadline | CaptureCase::CancellationBeforeDeadline
            ),
        );
        let sql = if matches!(case, CaptureCase::Unsupported) {
            "LISTEN channel"
        } else {
            "SELECT 1"
        };
        old_cassie.execute_sql_with_cancellation_converted(
            &old_session,
            sql,
            vec![],
            &old_cancellation,
            |_, _| Ok(()),
        )
    });
    contended_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("actual capture contention");
    let held_during_contention = held.load(Ordering::Acquire);

    // Act
    if !matches!(case, CaptureCase::Deadline) {
        cancellation.cancel();
    }
    if matches!(case, CaptureCase::Recovery) {
        cassie.midge.latch_storage_recovery_required_for_test();
    }
    release_tx.send(()).expect("release transaction lock");
    holder.join().expect("holder");
    let result = worker.join().expect("query worker");
    let current_owner = session.transaction_unique_owner_id();
    let failed = session.is_transaction_failed();
    let permit = cassie.runtime.try_begin_running_query();
    let permit_released = permit.is_some();
    drop(permit);
    session.rollback_transaction();

    // Assert
    assert!(held_during_contention);
    match case {
        CaptureCase::Cancellation | CaptureCase::CancellationBeforeDeadline => {
            assert!(matches!(result, Err(CassieError::QueryCanceled)));
        }
        CaptureCase::Deadline => assert!(matches!(result, Err(CassieError::DeadlineExceeded))),
        CaptureCase::Unsupported => assert!(
            matches!(result, Err(CassieError::Unsupported(ref message)) if message == "LISTEN is not supported")
        ),
        CaptureCase::EmptyUser => assert!(matches!(result, Err(CassieError::Unauthorized))),
        CaptureCase::MissingDatabase => assert!(
            matches!(result, Err(CassieError::CatalogObjectNotFound { ref name, .. }) if name == "missing_capture_db")
        ),
        CaptureCase::Recovery => assert!(
            matches!(result, Err(CassieError::Storage(ref message)) if message == "storage recovery required after an earlier unconfirmed write; this operation was not submitted; recover and reconcile before retrying")
        ),
    }
    assert_eq!(current_owner, owner);
    assert!(failed);
    assert!(permit_released);
}

#[test]
fn should_fail_current_owner_when_capture_wait_is_canceled() {
    const CASE: &str = "contended_capture_canceled";
    if !super::query_failure_watchdog::enter_child(
        "app::query_failure_admission_tests::should_fail_current_owner_when_capture_wait_is_canceled",
        CASE,
    ) {
        return;
    }
    // Arrange: The watchdog bounds this scenario.
    // Act: Run the synchronized scenario.
    // Assert: The helper checks its result before returning.
    assert_contended_capture_fails_its_current_owner(CaptureCase::Cancellation);
    super::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_fail_current_owner_when_capture_wait_exceeds_deadline() {
    const CASE: &str = "contended_capture_deadline";
    if !super::query_failure_watchdog::enter_child(
        "app::query_failure_admission_tests::should_fail_current_owner_when_capture_wait_exceeds_deadline",
        CASE,
    ) {
        return;
    }
    // Arrange: The watchdog bounds this scenario.
    // Act: Run the synchronized scenario.
    // Assert: The helper checks its result before returning.
    assert_contended_capture_fails_its_current_owner(CaptureCase::Deadline);
    super::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_preserve_unsupported_error_when_capture_wait_is_canceled() {
    const CASE: &str = "contended_capture_unsupported";
    if !super::query_failure_watchdog::enter_child(
        "app::query_failure_admission_tests::should_preserve_unsupported_error_when_capture_wait_is_canceled", CASE,
    ) { return; }
    // Arrange: The watchdog bounds this scenario.
    // Act: Run the synchronized scenario.
    // Assert: The helper checks its result before returning.
    assert_contended_capture_fails_its_current_owner(CaptureCase::Unsupported);
    super::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_preserve_empty_user_error_when_capture_wait_is_canceled() {
    const CASE: &str = "contended_capture_empty_user";
    if !super::query_failure_watchdog::enter_child(
        "app::query_failure_admission_tests::should_preserve_empty_user_error_when_capture_wait_is_canceled", CASE,
    ) { return; }
    // Arrange: The watchdog bounds this scenario.
    // Act: Run the synchronized scenario.
    // Assert: The helper checks its result before returning.
    assert_contended_capture_fails_its_current_owner(CaptureCase::EmptyUser);
    super::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_preserve_missing_database_error_when_capture_wait_is_canceled() {
    const CASE: &str = "contended_capture_missing_database";
    if !super::query_failure_watchdog::enter_child(
        "app::query_failure_admission_tests::should_preserve_missing_database_error_when_capture_wait_is_canceled", CASE,
    ) { return; }
    // Arrange: The watchdog bounds this scenario.
    // Act: Run the synchronized scenario.
    // Assert: The helper checks its result before returning.
    assert_contended_capture_fails_its_current_owner(CaptureCase::MissingDatabase);
    super::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_preserve_inner_recovery_error_when_capture_wait_is_canceled() {
    const CASE: &str = "contended_capture_inner_recovery";
    if !super::query_failure_watchdog::enter_child(
        "app::query_failure_admission_tests::should_preserve_inner_recovery_error_when_capture_wait_is_canceled", CASE,
    ) { return; }
    // Arrange: The watchdog bounds this scenario.
    // Act: Run the synchronized scenario.
    // Assert: The helper checks its result before returning.
    assert_contended_capture_fails_its_current_owner(CaptureCase::Recovery);
    super::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_prefer_cancellation_when_capture_wait_also_exceeds_deadline() {
    const CASE: &str = "contended_capture_cancel_deadline";
    if !super::query_failure_watchdog::enter_child(
        "app::query_failure_admission_tests::should_prefer_cancellation_when_capture_wait_also_exceeds_deadline", CASE,
    ) { return; }
    // Arrange: The watchdog bounds this scenario.
    // Act: Run the synchronized scenario.
    // Assert: The helper checks its result before returning.
    assert_contended_capture_fails_its_current_owner(CaptureCase::CancellationBeforeDeadline);
    super::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_capture_current_owner_after_query_admission() {
    const CASE: &str = "post_admission_owner_capture";
    if !super::query_failure_watchdog::enter_child(
        "app::query_failure_admission_tests::should_capture_current_owner_after_query_admission",
        CASE,
    ) {
        return;
    }

    // Arrange: the future observer pauses after the actual permit is acquired,
    // before root capture, then reports the actual captured owner and epoch.
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
        let _hook =
            super::query_lifecycle::authority_test_hooks::install_after_admission_before_capture(
                &old_cassie,
                &old_session,
                admitted_tx,
                resume_rx,
                captured_tx,
            );
        old_cassie.execute_sql_with_cancellation_converted(
            &old_session,
            "SELECT 1",
            vec![],
            &QueryCancellationHandle::new(),
            |_, _| Ok(()),
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
        .expect("actual capture");
    let result = worker.join().expect("query worker");
    let healthy = !session.is_transaction_failed();
    let current = session.transaction_unique_owner_id();
    session.rollback_transaction();

    // Assert: this documents the later linearization point, without treating a
    // stale operation's error against B as desired behavior.
    assert_ne!(owner_a, owner_b);
    assert_eq!(captured, Some((owner_b, epoch_b)));
    assert!(result.is_ok());
    assert!(healthy);
    assert_eq!(current, owner_b);
    drop(result);
    drop(session);
    drop(cassie);
    super::query_failure_watchdog::complete_child(CASE);
}

fn assert_explain_timeout_snapshot(initial: &str, changed: &str, expected_deadline: bool) {
    let mut config = CassieRuntimeConfig::default();
    config.limits.max_query_workers = 1;
    config.limits.query_timeout_ms = 0;
    let cassie = Cassie::new_in_memory_with_config(config).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    session
        .set_setting("statement_timeout", initial)
        .expect("initial timeout");
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
        .expect("transaction lock held");
    let (contended_tx, contended_rx) = mpsc::sync_channel(1);
    let (capture_tx, capture_rx) = mpsc::sync_channel(1);
    let old_cassie = cassie.clone();
    let old_session = session.clone();
    let worker = std::thread::spawn(move || {
        // Future hook must observe this EXPLAIN's real failed capture try-lock.
        let _hook = super::query_lifecycle::authority_test_hooks::install_paused_capture_contention(
            &old_cassie,
            &old_session,
            contended_tx,
            capture_rx,
        );
        old_cassie.explain_sql_with_cancellation_converted(
            &old_session,
            "SELECT 1",
            vec![],
            &QueryCancellationHandle::new(),
            |_, controls| Ok(controls.deadline.is_some()),
        )
    });
    contended_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("actual capture contention");
    let held_during_contention = held.load(Ordering::Acquire);

    // Act: the actual-root setting write waits until the prior root holder
    // retires, while the capturing worker remains at its real contention seam.
    release_tx.send(()).expect("release transaction lock");
    holder.join().expect("holder");
    session
        .set_setting("statement_timeout", changed)
        .expect("concurrent setting");
    capture_tx
        .send(())
        .expect("resume capture after setting change");
    let result = worker.join().expect("EXPLAIN worker");
    let current = session.transaction_unique_owner_id();
    let healthy = !session.is_transaction_failed();
    session.rollback_transaction();

    // Assert: converter observes the actual controls used by EXPLAIN.
    assert!(held_during_contention);
    assert_eq!(result.expect("EXPLAIN"), expected_deadline);
    assert_eq!(current, owner);
    assert!(healthy);
}

#[test]
fn should_preserve_disabled_explain_timeout_during_capture_wait() {
    const CASE: &str = "explain_capture_no_timeout";
    if !super::query_failure_watchdog::enter_child(
        "app::query_failure_admission_tests::should_preserve_disabled_explain_timeout_during_capture_wait", CASE,
    ) { return; }
    // Arrange: The watchdog bounds this scenario; the long timeout is not a latency assertion.
    // Act: Run the synchronized scenario.
    // Assert: The helper checks its result before returning.
    assert_explain_timeout_snapshot("0", "600000", false);
    super::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_preserve_enabled_explain_timeout_during_capture_wait() {
    const CASE: &str = "explain_capture_timeout";
    if !super::query_failure_watchdog::enter_child(
        "app::query_failure_admission_tests::should_preserve_enabled_explain_timeout_during_capture_wait", CASE,
    ) { return; }
    // Arrange: The watchdog bounds this scenario; the long timeout is not a latency assertion.
    // Act: Run the synchronized scenario.
    // Assert: The helper checks its result before returning.
    assert_explain_timeout_snapshot("600000", "0", true);
    super::query_failure_watchdog::complete_child(CASE);
}
