//! Focused converted-query completion authority probes.

use super::{Cassie, CassieError, CassieSession};
use crate::config::CassieRuntimeConfig;
use crate::runtime::QueryCancellationHandle;
use std::sync::mpsc;
use std::time::Duration;

fn fixture() -> (Cassie, CassieSession) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    (cassie, session)
}

#[test]
fn should_keep_replacement_healthy_after_old_explain_conversion_fails() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin A");
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = mpsc::sync_channel(1);
    let old_cassie = cassie.clone();
    let old_session = session.clone();
    let worker = std::thread::spawn(move || {
        old_cassie.explain_sql_with_cancellation_converted(
            &old_session,
            "SELECT 1",
            vec![],
            &QueryCancellationHandle::new(),
            move |_, _| {
                ready_tx.send(()).expect("explain conversion");
                resume_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("resume");
                Err::<(), _>(CassieError::Execution("old explain conversion".into()))
            },
        )
    });
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("conversion entered");
    session.rollback_transaction();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin B");

    // Act
    resume_tx.send(()).expect("resume explain");
    let old = worker.join().expect("explain worker");
    let healthy = !session.is_transaction_failed();
    let result = cassie.execute_sql(&session, "SELECT 1", vec![]);
    session.rollback_transaction();

    // Assert
    assert!(
        matches!(old, Err(CassieError::Execution(ref message)) if message == "old explain conversion")
    );
    assert!(healthy);
    assert!(result.is_ok());
}

#[test]
fn should_fail_current_transaction_after_explain_conversion_error() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin");

    // Act
    let result: Result<(), CassieError> = cassie.explain_sql_with_cancellation_converted(
        &session,
        "SELECT 1",
        vec![],
        &QueryCancellationHandle::new(),
        |_, _| Err(CassieError::Execution("explain conversion".into())),
    );
    let failed = session.is_transaction_failed();
    session.rollback_transaction();

    // Assert
    assert!(
        matches!(result, Err(CassieError::Execution(ref message)) if message == "explain conversion")
    );
    assert!(failed);
}

#[test]
fn should_preserve_independent_implicit_owner_after_old_begin_promotes() {
    const CASE: &str = "independent_implicit_promotion";
    if !super::query_failure_watchdog::enter_child(
        "app::query_failure_converted_tests::should_preserve_independent_implicit_owner_after_old_begin_promotes",
        CASE,
    ) {
        return;
    }

    // Arrange: the future hook must observe A's actual captured Idle authority.
    let (cassie, session) = fixture();
    let old_cassie = cassie.clone();
    let old_session = session.clone();
    let (captured_tx, captured_rx) = mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let _pause = super::query_lifecycle::authority_test_hooks::install_after_root_capture(
            &old_cassie,
            &old_session,
            captured_tx,
            resume_rx,
        );
        old_cassie.execute_sql_with_cancellation_converted(
            &old_session,
            "BEGIN",
            vec![],
            &QueryCancellationHandle::new(),
            |_, _| Err::<(), _>(CassieError::Execution("old begin conversion".into())),
        )
    });
    let captured = captured_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("old root authority captured");
    assert_eq!(captured, None);
    session.begin_implicit_transaction().expect("independent B");
    let owner_b = session.transaction_unique_owner_id();

    // Act
    resume_tx.send(()).expect("resume old begin");
    let result = worker.join().expect("old begin worker");
    let current_owner = session.transaction_unique_owner_id();
    let active = session.is_transaction_active();
    let failed = session.is_transaction_failed();
    let implicit = session.is_implicit_transaction();
    let usable = cassie.execute_sql(&session, "SELECT 1", vec![]);
    session.rollback_transaction();

    // Assert: existing BEGIN promotion changes B's origin; this probes only
    // whether old A's conversion error is allowed to mark B failed.
    assert!(
        matches!(result, Err(CassieError::Execution(ref message)) if message == "old begin conversion")
    );
    assert_eq!(current_owner, owner_b);
    assert!(active);
    assert!(!failed);
    assert!(!implicit);
    assert!(usable.is_ok());
    drop(usable);
    drop(result);
    drop(session);
    drop(cassie);
    super::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_keep_replacement_healthy_after_old_copy_predicate_is_canceled() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin A");
    let (_pause, entered, resume) = crate::runtime::pause_next_query_at_sleep_entry();
    let cancellation = QueryCancellationHandle::new();
    let controls = crate::runtime::QueryExecutionControls::with_cancellation(
        &crate::config::CassieRuntimeLimits::default(),
        std::time::Instant::now(),
        cancellation.clone(),
    );
    let old_cassie = cassie.clone();
    let old_session = session.clone();
    let worker = std::thread::spawn(move || {
        old_cassie.execute_copy_where_with_controls(
            &old_session,
            "SELECT pg_sleep(0)",
            vec![],
            &controls,
        )
    });
    entered
        .recv_timeout(Duration::from_secs(5))
        .expect("predicate entered");
    session.rollback_transaction();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin B");

    // Act
    cancellation.cancel();
    resume.send(()).expect("resume predicate");
    let old = worker.join().expect("predicate worker");
    let healthy = !session.is_transaction_failed();
    let result = cassie.execute_sql(&session, "SELECT 1", vec![]);
    session.rollback_transaction();

    // Assert
    assert!(matches!(old, Err(CassieError::QueryCanceled)));
    assert!(healthy);
    assert!(result.is_ok());
}

#[test]
fn should_fail_current_transaction_after_copy_predicate_cancellation() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin");
    let cancellation = QueryCancellationHandle::new();
    let controls = crate::runtime::QueryExecutionControls::with_cancellation(
        &crate::config::CassieRuntimeLimits::default(),
        std::time::Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();

    // Act
    let result = cassie.execute_copy_where_with_controls(&session, "SELECT 1", vec![], &controls);
    let failed = session.is_transaction_failed();
    session.rollback_transaction();

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert!(failed);
}

#[test]
fn should_keep_operations_distinct_when_cancellation_handle_is_reused() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin A");
    let cancellation = QueryCancellationHandle::new();
    let old_cancellation = cancellation.clone();
    let old_cassie = cassie.clone();
    let old_session = session.clone();
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        old_cassie.execute_sql_with_cancellation_converted(
            &old_session,
            "SELECT 1",
            vec![],
            &old_cancellation,
            move |_, _| {
                entered_tx.send(()).expect("old conversion");
                resume_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("resume");
                Err::<(), _>(CassieError::Execution("old conversion failure".into()))
            },
        )
    });
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("old query entered");
    session.rollback_transaction();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin B");
    let second: Result<(), CassieError> = cassie.execute_sql_with_cancellation_converted(
        &session,
        "SELECT 1",
        vec![],
        &cancellation,
        |_, _| Err(CassieError::Execution("new conversion failure".into())),
    );
    let b_failed = session.is_transaction_failed();
    session.rollback_transaction();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin C");
    let owner_c = session.transaction_unique_owner_id();

    // Act
    resume_tx.send(()).expect("resume old conversion");
    let first = worker.join().expect("old worker");
    let healthy = !session.is_transaction_failed();
    let current_owner = session.transaction_unique_owner_id();
    let usable = cassie.execute_sql(&session, "SELECT 1", vec![]);
    session.rollback_transaction();

    // Assert
    assert!(
        matches!(second, Err(CassieError::Execution(ref message)) if message == "new conversion failure")
    );
    assert!(b_failed);
    assert!(
        matches!(first, Err(CassieError::Execution(ref message)) if message == "old conversion failure")
    );
    assert!(healthy);
    assert_eq!(current_owner, owner_c);
    assert!(usable.is_ok());
}

#[test]
fn should_keep_newer_operation_healthy_when_shared_handle_outlives_old_query() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin A");
    let owner_a = session.transaction_unique_owner_id();
    let cancellation = QueryCancellationHandle::new();
    let old_cancellation = cancellation.clone();
    let old_cassie = cassie.clone();
    let old_session = session.clone();
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        old_cassie.execute_sql_with_cancellation_converted(
            &old_session,
            "SELECT 1",
            vec![],
            &old_cancellation,
            move |_, _| {
                entered_tx.send(()).expect("old conversion");
                resume_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("resume");
                Err::<(), _>(CassieError::Execution("old conversion failure".into()))
            },
        )
    });
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("old query entered");
    session.rollback_transaction();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin B");
    let owner_b = session.transaction_unique_owner_id();
    let second = cassie.execute_sql_with_cancellation_converted(
        &session,
        "SELECT 1",
        vec![],
        &cancellation,
        |_, _| Ok(()),
    );

    // Act
    resume_tx.send(()).expect("resume old conversion");
    let first = worker.join().expect("old worker");
    let healthy = !session.is_transaction_failed();
    let current_owner = session.transaction_unique_owner_id();
    let usable = cassie.execute_sql(&session, "SELECT 1", vec![]);
    session.rollback_transaction();

    // Assert
    assert!(second.is_ok());
    assert!(
        matches!(first, Err(CassieError::Execution(ref message)) if message == "old conversion failure")
    );
    assert!(healthy);
    assert_ne!(owner_a, owner_b);
    assert_eq!(current_owner, owner_b);
    assert!(usable.is_ok());
}

#[test]
fn should_fail_current_transaction_after_prepared_conversion_error() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(&session, "PREPARE authority_p AS SELECT 1", vec![])
        .expect("prepare");
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin");

    // Act
    let result: Result<(), CassieError> = cassie.execute_sql_with_cancellation_converted(
        &session,
        "EXECUTE authority_p",
        vec![],
        &QueryCancellationHandle::new(),
        |_, _| Err(CassieError::Execution("prepared conversion".into())),
    );
    let failed = session.is_transaction_failed();
    session.rollback_transaction();

    // Assert
    assert!(
        matches!(result, Err(CassieError::Execution(ref message)) if message == "prepared conversion")
    );
    assert!(failed);
}

#[test]
fn should_fail_current_transaction_after_prepared_body_error() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(
            &session,
            "PREPARE authority_divide (INT) AS SELECT 10 / $1",
            vec![],
        )
        .expect("prepare");
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin");

    // Act
    let result = cassie.execute_sql(&session, "EXECUTE authority_divide(0)", vec![]);
    let failed = session.is_transaction_failed();
    session.rollback_transaction();

    // Assert
    assert!(
        matches!(result, Err(CassieError::Execution(ref message)) if message == "division by zero")
    );
    assert!(failed);
}

#[test]
fn should_keep_replacement_healthy_after_old_cursor_conversion_error() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin A");
    let owner_a = session.transaction_unique_owner_id();
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = mpsc::sync_channel(1);
    let old_cassie = cassie.clone();
    let old_session = session.clone();
    let worker = std::thread::spawn(move || {
        old_cassie.execute_sql_with_cancellation_converted(
            &old_session,
            "DECLARE authority_cursor CURSOR FOR SELECT 1",
            vec![],
            &QueryCancellationHandle::new(),
            move |_, _| {
                entered_tx.send(()).expect("cursor conversion");
                resume_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("resume");
                Err::<(), _>(CassieError::Execution("cursor conversion".into()))
            },
        )
    });
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("cursor conversion entered");
    session.rollback_transaction();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin B");
    let owner_b = session.transaction_unique_owner_id();
    cassie
        .execute_sql(
            &session,
            "DECLARE replacement_cursor CURSOR FOR SELECT 2",
            vec![],
        )
        .expect("B cursor");

    // Act
    resume_tx.send(()).expect("resume cursor");
    let result = worker.join().expect("cursor worker");
    let current_owner = session.transaction_unique_owner_id();
    let healthy = !session.is_transaction_failed();
    let usable = cassie.execute_sql(&session, "FETCH ALL FROM replacement_cursor", vec![]);
    session.rollback_transaction();

    // Assert
    assert!(
        matches!(result, Err(CassieError::Execution(ref message)) if message == "cursor conversion")
    );
    assert_ne!(owner_a, owner_b);
    assert_eq!(current_owner, owner_b);
    assert!(healthy);
    assert_eq!(
        usable.expect("B cursor survives").rows(),
        &[vec![crate::types::Value::Int64(2)]]
    );
}
