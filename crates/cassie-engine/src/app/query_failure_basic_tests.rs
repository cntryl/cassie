//! Transaction authority for query failure and response conversion.

use super::{Cassie, CassieError, CassieSession};
use crate::config::CassieRuntimeConfig;
use crate::runtime::QueryCancellationHandle;
use crate::types::Value;
use std::sync::mpsc;
use std::time::Duration;

fn fixture() -> (Cassie, CassieSession) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE failure_owner_rows (id INT)", vec![])
        .expect("table");
    (cassie, session)
}

fn delayed_conversion_error(
    cassie: &Cassie,
    session: &CassieSession,
) -> (
    mpsc::Receiver<()>,
    mpsc::SyncSender<()>,
    std::thread::JoinHandle<Result<(), CassieError>>,
) {
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = mpsc::sync_channel(1);
    let worker_cassie = cassie.clone();
    let worker_session = session.clone();
    let worker = std::thread::spawn(move || {
        worker_cassie.execute_sql_with_cancellation_converted(
            &worker_session,
            "SELECT 1",
            vec![],
            &QueryCancellationHandle::new(),
            move |_, _| {
                ready_tx.send(()).expect("conversion boundary");
                resume_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("resume conversion");
                Err(CassieError::Execution("injected conversion failure".into()))
            },
        )
    });
    (ready_rx, resume_tx, worker)
}

#[test]
fn should_keep_replacement_transaction_healthy_after_old_conversion_fails() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin A");
    let (ready, resume, worker) = delayed_conversion_error(&cassie, &session);
    ready
        .recv_timeout(Duration::from_secs(5))
        .expect("old conversion");
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("retire A");
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin B");

    // Act
    resume.send(()).expect("finish old query");
    let old = worker.join().expect("old worker");
    let healthy = !session.is_transaction_failed();
    let insert = cassie.execute_sql(
        &session,
        "INSERT INTO failure_owner_rows VALUES (7)",
        vec![],
    );
    let commit = cassie.execute_sql(&session, "COMMIT", vec![]);
    let rows = cassie.execute_sql(&session, "SELECT id FROM failure_owner_rows", vec![]);

    // Assert
    assert!(
        matches!(old, Err(CassieError::Execution(ref message)) if message == "injected conversion failure")
    );
    assert!(healthy);
    assert!(insert.is_ok());
    assert!(commit.is_ok());
    assert_eq!(
        rows.expect("committed row").rows(),
        &[vec![Value::Int64(7)]]
    );
}

#[test]
fn should_keep_independent_transaction_healthy_after_idle_query_fails() {
    // Arrange
    let (cassie, session) = fixture();
    let (ready, resume, worker) = delayed_conversion_error(&cassie, &session);
    ready
        .recv_timeout(Duration::from_secs(5))
        .expect("idle conversion");
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("independent transaction");

    // Act
    resume.send(()).expect("finish old query");
    let old = worker.join().expect("old worker");
    let healthy = !session.is_transaction_failed();
    let result = cassie.execute_sql(&session, "SELECT 1", vec![]);
    session.rollback_transaction();

    // Assert
    assert!(
        matches!(old, Err(CassieError::Execution(ref message)) if message == "injected conversion failure")
    );
    assert!(healthy);
    assert!(result.is_ok());
}

#[test]
fn should_fail_current_transaction_after_conversion_error() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin");

    // Act
    let result: Result<(), CassieError> = cassie.execute_sql_with_cancellation_converted(
        &session,
        "SELECT 1",
        vec![],
        &QueryCancellationHandle::new(),
        |_, _| Err(CassieError::Execution("injected conversion failure".into())),
    );
    let failed = session.is_transaction_failed();
    session.rollback_transaction();

    // Assert
    assert!(
        matches!(result, Err(CassieError::Execution(ref message)) if message == "injected conversion failure")
    );
    assert!(failed);
}

#[test]
fn should_fail_current_transaction_after_conversion_cancellation() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin");
    let cancellation = QueryCancellationHandle::new();
    let conversion_cancellation = cancellation.clone();

    // Act
    let result: Result<(), CassieError> = cassie.execute_sql_with_cancellation_converted(
        &session,
        "SELECT 1",
        vec![],
        &cancellation,
        move |_, controls| {
            conversion_cancellation.cancel();
            controls.check_interruption()
        },
    );
    let failed = session.is_transaction_failed();
    session.rollback_transaction();

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert!(failed);
}

#[test]
fn should_fail_transaction_started_by_own_begin_after_conversion_error() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let result: Result<(), CassieError> = cassie.execute_sql_with_cancellation_converted(
        &session,
        "BEGIN",
        vec![],
        &QueryCancellationHandle::new(),
        |_, _| Err(CassieError::Execution("injected conversion failure".into())),
    );
    let failed = session.is_transaction_failed();
    session.rollback_transaction();

    // Assert
    assert!(
        matches!(result, Err(CassieError::Execution(ref message)) if message == "injected conversion failure")
    );
    assert!(failed);
}

#[test]
fn should_fail_transaction_started_by_own_chain_after_conversion_error() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin");
    let old_generation = session.transaction_generation();

    // Act
    let result: Result<(), CassieError> = cassie.execute_sql_with_cancellation_converted(
        &session,
        "COMMIT AND CHAIN",
        vec![],
        &QueryCancellationHandle::new(),
        |_, _| Err(CassieError::Execution("injected conversion failure".into())),
    );
    let generation = session.transaction_generation();
    let failed = session.is_transaction_failed();
    session.rollback_transaction();

    // Assert
    assert!(
        matches!(result, Err(CassieError::Execution(ref message)) if message == "injected conversion failure")
    );
    assert_ne!(generation, old_generation);
    assert!(failed);
}

#[test]
fn should_preserve_transaction_authority_during_implicit_promotion() {
    // Arrange
    let (cassie, session) = fixture();
    session
        .begin_implicit_transaction()
        .expect("implicit begin");
    let generation = session.transaction_generation();

    // Act
    let result: Result<(), CassieError> = cassie.execute_sql_with_cancellation_converted(
        &session,
        "BEGIN",
        vec![],
        &QueryCancellationHandle::new(),
        |_, _| Err(CassieError::Execution("injected conversion failure".into())),
    );
    let promoted_generation = session.transaction_generation();
    let failed = session.is_transaction_failed();
    session.rollback_transaction();

    // Assert
    assert!(
        matches!(result, Err(CassieError::Execution(ref message)) if message == "injected conversion failure")
    );
    assert_eq!(promoted_generation, generation);
    assert!(failed);
}
