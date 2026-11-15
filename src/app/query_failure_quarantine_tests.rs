//! Authored, unrun recovery-admission precedence witnesses.

use super::{Cassie, CassieError};
use crate::config::CassieRuntimeConfig;
use crate::runtime::QueryCancellationHandle;
use std::sync::mpsc;
use std::time::Duration;

fn fixture() -> (Cassie, super::CassieSession) {
    let mut config = CassieRuntimeConfig::default();
    config.limits.max_query_workers = 1;
    let cassie = Cassie::new_in_memory_with_config(config).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    (cassie, session)
}

#[test]
fn should_preserve_recovery_error_for_canceled_saturated_query() {
    // Arrange
    let (cassie, session) = fixture();
    session.begin_transaction(None).expect("begin");
    let occupied = cassie
        .runtime
        .try_begin_running_query()
        .expect("occupy slot");
    cassie.midge.latch_storage_recovery_required_for_test();
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();

    // Act
    let result = cassie.execute_sql_with_cancellation_converted(
        &session,
        "SELECT 1",
        vec![],
        &cancellation,
        |_, _| Ok(()),
    );
    let failed = session.is_transaction_failed();
    drop(occupied);
    session.rollback_transaction();

    // Assert: recovery currently wins over admission and cancellation.
    let error = result.expect_err("recovery required");
    assert!(matches!(&error, CassieError::Storage(message)
        if message == "storage recovery required after an earlier unconfirmed write; this operation was not submitted; recover and reconcile before retrying"));
    assert_eq!(error.sql_state(), "58030");
    assert!(failed);
}

#[test]
fn should_preserve_admission_result_when_recovery_latches_after_check() {
    const CASE: &str = "recovery_after_admission_check";
    if !super::query_failure_watchdog::enter_child(
        "app::query_failure_quarantine_tests::should_preserve_admission_result_when_recovery_latches_after_check",
        CASE,
    ) {
        return;
    }

    // Arrange: future hook reports the actual original quarantine helper's
    // None result (not a second precheck), then pauses with its locks released
    // before the original nonblocking running-query admission.
    let (cassie, session) = fixture();
    let occupied = cassie
        .runtime
        .try_begin_running_query()
        .expect("occupy slot");
    let old_cassie = cassie.clone();
    let old_session = session.clone();
    let (checked_tx, checked_rx) = mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let _pause =
            super::query_lifecycle::authority_test_hooks::install_after_healthy_recovery_check(
                &old_cassie,
                &old_session,
                checked_tx,
                resume_rx,
            );
        old_cassie.execute_sql_with_cancellation_converted(
            &old_session,
            "SELECT 1",
            vec![],
            &QueryCancellationHandle::new(),
            |_, _| Ok(()),
        )
    });
    checked_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("healthy recovery check");
    cassie.midge.latch_storage_recovery_required_for_test();

    // Act
    resume_tx.send(()).expect("resume admission");
    let result = worker.join().expect("query worker");
    drop(occupied);

    // Assert: the single original check preceded the later latch.
    assert!(
        matches!(result, Err(CassieError::Execution(ref message)) if message == "query admission exhausted")
    );
    drop(result);
    drop(session);
    drop(cassie);
    super::query_failure_watchdog::complete_child(CASE);
}
