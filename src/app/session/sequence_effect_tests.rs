//! Sequence admission is owned before a native cursor update can start.

use crate::app::{Cassie, CassieSession};
use crate::config::CassieRuntimeConfig;
#[cfg(feature = "replay-benchmark-observation")]
use crate::midge::adapter::observe_native_commits;
use crate::types::Value;
use std::sync::mpsc;
use std::time::Duration;

fn fixture() -> (Cassie, CassieSession) {
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("sequence_owner", None);
    for sql in [
        "CREATE SEQUENCE effect_sequence",
        "CREATE TABLE effect_rows (value BIGINT DEFAULT nextval('effect_sequence'))",
        "BEGIN",
        "SET LOCAL cassie.projection_write_mode = 'replayable'",
    ] {
        cassie.execute_sql(&session, sql, vec![]).unwrap();
    }
    (cassie, session)
}

#[test]
fn should_reject_sequence_effects_captured_before_the_commit_seal() {
    for sql in [
        "SELECT nextval('effect_sequence')",
        "SELECT setval('effect_sequence', 42, false)",
        "INSERT INTO effect_rows DEFAULT VALUES",
    ] {
        // Arrange
        let (cassie, session) = fixture();
        let (captured_tx, captured_rx) = mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        let worker_cassie = cassie.clone();
        let worker_session = session.clone();
        let worker = std::thread::spawn(move || {
            let _pause =
                crate::app::query_lifecycle::authority_test_hooks::install_after_root_capture(
                    &worker_cassie,
                    &worker_session,
                    captured_tx,
                    resume_rx,
                );
            worker_cassie.execute_sql(&worker_session, sql, vec![])
        });
        captured_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let seal = session.seal_transaction_for_commit();

        // Act
        resume_tx.send(()).unwrap();
        let rejected = worker.join().unwrap();
        drop(seal);
        let other = cassie.create_session("sequence_observer", None);
        let cursor = cassie.execute_sql(&other, "SELECT nextval('effect_sequence')", vec![]);

        // Assert
        assert!(rejected.is_err(), "late sequence effect: {sql}");
        assert_eq!(cursor.unwrap().rows()[0][0], Value::Int64(1));
        session.rollback_transaction();
    }
}

#[test]
fn should_reject_a_sequence_effect_after_its_transaction_is_replaced() {
    // Arrange
    let (_cassie, session) = fixture();
    let authority = super::FailureAuthority::capture(&session);
    let operation = session.with_operation_failure_authority(Some(&authority));
    session.rollback_transaction();
    session.begin_transaction(None).unwrap();

    // Act
    let admitted = operation.admit_sequence_projection_effect();

    // Assert
    assert!(admitted.is_err());
    assert_eq!(
        session.projection_commit_policy(false),
        super::ProjectionCommitPolicy::Existing
    );
}

#[test]
fn should_reject_a_detached_sequence_effect_without_root_lineage() {
    // Arrange
    let (cassie, session) = fixture();
    let batch = cassie.new_statement_batch(Some(&session)).unwrap();
    session.rollback_transaction();
    session.begin_transaction(None).unwrap();

    // Act
    let admitted = batch.session().admit_sequence_projection_effect();

    // Assert
    assert!(admitted.is_err());
    assert_eq!(
        session.projection_commit_policy(false),
        super::ProjectionCommitPolicy::Existing
    );
}

#[test]
fn should_reject_an_idle_sequence_owner_after_an_independent_begin() {
    // Arrange
    let session = CassieSession::new("idle_sequence_owner".into(), None);
    let authority = super::FailureAuthority::capture(&session);
    let operation = session.with_operation_failure_authority(Some(&authority));
    session.begin_transaction(None).unwrap();

    // Act
    let admitted = operation.admit_sequence_projection_effect();

    // Assert
    assert!(admitted.is_err());
}

#[test]
fn should_adopt_an_own_begin_before_sequence_admission() {
    // Arrange
    let session = CassieSession::new("own_sequence_owner".into(), None);
    let authority = super::FailureAuthority::capture(&session);
    let operation = session.with_operation_failure_authority(Some(&authority));
    session
        .begin_transaction_with_modes_for_operation(None, Default::default(), Some(&authority))
        .unwrap();

    // Act
    let admitted = operation.admit_sequence_projection_effect();

    // Assert
    assert!(admitted.is_ok());
}

#[test]
fn should_allow_committing_root_sequence_resolution_but_reject_a_same_thread_fork() {
    // Arrange
    let (cassie, session) = fixture();
    let authority = super::FailureAuthority::capture(&session);
    let operation = session.with_operation_failure_authority(Some(&authority));
    let batch = cassie.new_statement_batch(Some(&operation)).unwrap();
    let seal = session.seal_transaction_for_commit();

    // Act
    let root = operation.admit_sequence_projection_effect();
    let fork = batch.session().admit_sequence_projection_effect();

    // Assert
    assert!(root.is_ok());
    assert!(fork.is_err());
    drop(seal);
}

#[test]
fn should_reject_a_sequence_effect_from_a_restored_savepoint_epoch() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(&session, "SAVEPOINT before_sequence", vec![])
        .unwrap();
    let authority = super::FailureAuthority::capture(&session);
    let operation = session.with_operation_failure_authority(Some(&authority));
    cassie
        .execute_sql(&session, "ROLLBACK TO before_sequence", vec![])
        .unwrap();

    // Act
    let admitted = operation.admit_sequence_projection_effect();

    // Assert
    assert!(admitted.is_err());
    assert_eq!(
        session.projection_commit_policy(false),
        super::ProjectionCommitPolicy::Replayable
    );
}

#[test]
#[cfg(feature = "replay-benchmark-observation")]
fn should_promote_data_policy_before_an_admitted_sequence_effect_reaches_storage() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(
            &session,
            "INSERT INTO effect_rows (value) VALUES (7)",
            vec![],
        )
        .unwrap();
    let (admitted_tx, admitted_rx) = mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = mpsc::sync_channel(1);
    let worker_cassie = cassie.clone();
    let worker_session = session.clone();
    let worker = std::thread::spawn(move || {
        let _pause =
            super::sequence_effect::test_hooks::pause_after_admission(admitted_tx, resume_rx);
        worker_cassie.execute_sql(&worker_session, "SELECT nextval('effect_sequence')", vec![])
    });
    admitted_rx.recv_timeout(Duration::from_secs(5)).unwrap();

    // Act
    let (commit, writes) =
        observe_native_commits(|| cassie.execute_sql(&session, "COMMIT", vec![]));
    resume_tx.send(()).unwrap();
    let sequence = worker.join().unwrap();

    // Assert
    commit.unwrap();
    assert_eq!(writes.first_policy, Some("local_sync"));
    assert_eq!(writes.primary_buffered_attempts, 0);
    assert_eq!(sequence.unwrap().rows()[0][0], Value::Int64(1));
}
