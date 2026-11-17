//! An outer wire completion uses its admitted operation, never the next block.

use super::{CassieSession, FailureAuthority};

#[test]
fn should_leave_a_block_healthy_when_wire_admission_rejects_before_capture() {
    // Arrange
    let session = CassieSession::new("wire_owner".into(), None);
    session.begin_transaction(None).unwrap();
    let operation = session.with_new_wire_operation();
    operation.reject_operation_admission();

    // Act
    operation.fail_operation_transaction();

    // Assert
    assert!(session.is_transaction_active());
    assert!(!session.is_transaction_failed());
}

#[test]
fn should_not_capture_a_protocol_owner_after_an_unowned_query_attempt() {
    // Arrange
    let session = CassieSession::new("unowned_wire".into(), None);
    session.begin_transaction(None).unwrap();
    let operation = session.with_new_wire_operation();
    operation.note_operation_worker_started();
    session.rollback_transaction();
    session.begin_transaction(None).unwrap();

    // Act
    let may_capture = operation.may_capture_protocol_owner();
    operation.fail_operation_transaction();

    // Assert
    assert!(!may_capture);
    assert!(!session.is_transaction_failed());
}

#[test]
fn should_preserve_a_replacement_after_an_admitted_wire_operation_fails() {
    // Arrange
    let session = CassieSession::new("wire_owner".into(), None);
    session.begin_transaction(None).unwrap();
    let operation = session.with_new_wire_operation();
    let _authority = FailureAuthority::capture_for_operation(&operation);
    session.rollback_transaction();
    session.begin_transaction(None).unwrap();

    // Act
    operation.fail_operation_transaction();

    // Assert
    assert!(session.is_transaction_active());
    assert!(!session.is_transaction_failed());
}

#[test]
fn should_rollback_only_the_owned_implicit_wire_transaction() {
    // Arrange
    let session = CassieSession::new("wire_owner".into(), None);
    let operation = session.with_new_wire_operation();
    let authority = FailureAuthority::capture_for_operation(&operation);
    session
        .begin_implicit_transaction_for_operation(Some(&authority))
        .unwrap();

    // Act
    operation.fail_operation_transaction();

    // Assert
    assert!(!session.is_transaction_active());
    assert!(!session.is_transaction_failed());
}

#[test]
fn should_fail_a_wire_operations_own_begin_after_adoption() {
    // Arrange
    let session = CassieSession::new("wire_owner".into(), None);
    let operation = session.with_new_wire_operation();
    let authority = FailureAuthority::capture_for_operation(&operation);
    session
        .begin_transaction_with_modes_for_operation(None, Default::default(), Some(&authority))
        .unwrap();

    // Act
    operation.fail_operation_transaction();

    // Assert
    assert!(session.is_transaction_failed());
}

#[test]
fn should_not_clear_portals_for_a_replacement_after_owned_retirement() {
    // Arrange
    let session = CassieSession::new("wire_owner".into(), None);
    session.begin_implicit_transaction().unwrap();
    let operation = session.with_new_wire_operation();
    let _authority = FailureAuthority::capture_for_operation(&operation);
    operation.fail_operation_transaction();
    session.begin_transaction(None).unwrap();
    let mut cleared = false;

    // Act
    operation.try_with_owned_idle_transaction(|| cleared = true);

    // Assert
    assert!(!cleared);
    assert!(!session.is_transaction_failed());
}

#[test]
fn should_refuse_a_stale_outer_commit_without_publishing_or_rolling_back_the_replacement() {
    // Arrange
    let cassie = crate::app::Cassie::new_in_memory_with_config(
        crate::config::CassieRuntimeConfig::default(),
    )
    .unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("outer_commit", None);
    cassie
        .execute_sql(&session, "CREATE TABLE outer_commit_rows (id INT)", vec![])
        .unwrap();
    session.begin_implicit_transaction().unwrap();
    let operation = session.with_new_wire_operation();
    let _authority = FailureAuthority::capture_for_operation(&operation);
    session.rollback_transaction();
    session.begin_implicit_transaction().unwrap();
    cassie
        .execute_sql(&session, "INSERT INTO outer_commit_rows VALUES (7)", vec![])
        .unwrap();

    // Act
    let stale_commit = cassie.commit_transaction(&operation);
    let replacement_healthy = session.is_transaction_active() && !session.is_transaction_failed();
    session.rollback_transaction();
    let rows = cassie
        .execute_sql(&session, "SELECT id FROM outer_commit_rows", vec![])
        .unwrap();

    // Assert
    assert!(stale_commit.is_err());
    assert!(replacement_healthy);
    assert!(rows.rows().is_empty());
}
