//! An outer wire completion uses its admitted operation, never the next block.

use super::{CassieSession, FailureAuthority};
use std::cell::RefCell;
use std::sync::mpsc::{Receiver, SyncSender};

thread_local! {
    static OWN_START: RefCell<Option<(SyncSender<()>, Receiver<()>)>> = const { RefCell::new(None) };
}

pub(crate) struct OwnedStartPause;
impl Drop for OwnedStartPause {
    fn drop(&mut self) {
        OWN_START.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}

impl CassieSession {
    pub(crate) fn pause_owned_transaction_start_for_test(
        entered: SyncSender<()>,
        resume: Receiver<()>,
    ) -> OwnedStartPause {
        OWN_START.with(|slot| assert!(slot.replace(Some((entered, resume))).is_none()));
        OwnedStartPause
    }

    pub(crate) fn observe_owned_transaction_start_for_test(&self) {
        if let Some((entered, resume)) = OWN_START.with(|slot| slot.borrow_mut().take()) {
            entered.send(()).unwrap();
            resume
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
        }
    }
}

#[test]
fn should_not_retire_a_replacement_when_an_owning_nested_statement_fails() {
    for sql in [
        "CREATE TABLE old_owned_ctas AS SELECT 1 / 0 AS value",
        "WITH inserted AS (INSERT INTO owned_nested_rows VALUES (7) RETURNING id) SELECT id / (id - id) FROM inserted",
    ] {
        // Arrange
        let cassie = crate::app::Cassie::new_in_memory_with_config(crate::config::CassieRuntimeConfig::default()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("owned_nested", None);
        cassie.execute_sql(&session, "CREATE TABLE owned_nested_rows (id INT)", vec![]).unwrap();
        let worker_cassie = cassie.clone();
        let worker_session = session.clone();
        let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let _pause = CassieSession::pause_owned_transaction_start_for_test(entered_tx, resume_rx);
            worker_cassie.execute_sql(&worker_session, sql, vec![])
        });
        entered_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        session.rollback_transaction();
        session.begin_transaction(None).unwrap();
        let owner_b = session.transaction_unique_owner_id();

        // Act
        resume_tx.send(()).unwrap();
        let old = worker.join().unwrap();

        // Assert
        assert_eq!(old.unwrap_err().sql_state(), "25000", "old owning statement: {sql}");
        assert!(session.is_transaction_active());
        assert!(!session.is_transaction_failed());
        assert_eq!(session.transaction_unique_owner_id(), owner_b);
        assert!(cassie.execute_sql(&session, "SELECT id FROM owned_nested_rows", vec![]).unwrap().rows().is_empty());
    }
}

#[test]
fn should_reject_an_old_insert_before_it_can_fork_a_replacement_transaction() {
    for read_only in [false, true] {
        // Arrange
        let cassie = crate::app::Cassie::new_in_memory_with_config(
            crate::config::CassieRuntimeConfig::default(),
        )
        .unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("insert_owner", None);
        cassie
            .execute_sql(&session, "CREATE TABLE old_insert_rows (id INT)", vec![])
            .unwrap();
        session.begin_transaction(None).unwrap();
        let worker_cassie = cassie.clone();
        let worker_session = session.clone();
        let (captured_tx, captured_rx) = std::sync::mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let _pause =
                crate::app::query_lifecycle::authority_test_hooks::install_after_root_capture(
                    &worker_cassie,
                    &worker_session,
                    captured_tx,
                    resume_rx,
                );
            worker_cassie.execute_sql(
                &worker_session,
                "INSERT INTO old_insert_rows VALUES (7)",
                vec![],
            )
        });
        captured_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        session.rollback_transaction();
        session
            .begin_transaction_with_modes(
                None,
                crate::sql::ast::TransactionModes {
                    read_only: Some(read_only),
                    ..Default::default()
                },
            )
            .unwrap();

        // Act
        resume_tx.send(()).unwrap();
        let old = worker.join().unwrap();
        let replacement_healthy =
            session.is_transaction_active() && !session.is_transaction_failed();
        cassie.execute_sql(&session, "COMMIT", vec![]).unwrap();
        let rows = cassie
            .execute_sql(&session, "SELECT id FROM old_insert_rows", vec![])
            .unwrap();

        // Assert
        assert_eq!(old.unwrap_err().sql_state(), "25000");
        assert!(replacement_healthy);
        assert!(rows.rows().is_empty());
    }
}

#[test]
fn should_not_declare_projection_mode_on_a_replacement_through_an_old_operation() {
    // Arrange
    let session = CassieSession::new("mode_owner".into(), None);
    session.begin_transaction(None).unwrap();
    let authority = FailureAuthority::capture(&session);
    let operation = session.with_operation_failure_authority(Some(&authority));
    session.rollback_transaction();
    session.begin_transaction(None).unwrap();

    // Act
    let declared = operation.set_projection_write_mode(Some("replayable"));

    // Assert
    assert_eq!(declared.unwrap_err().sql_state(), "25000");
    assert_eq!(session.projection_write_mode().as_str(), "default");
}

#[test]
fn should_refuse_an_old_sql_rollback_after_a_replacement_begins() {
    // Arrange
    let cassie = crate::app::Cassie::new_in_memory_with_config(
        crate::config::CassieRuntimeConfig::default(),
    )
    .unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("rollback_owner", None);
    session.begin_transaction(None).unwrap();
    let worker_cassie = cassie.clone();
    let worker_session = session.clone();
    let (captured_tx, captured_rx) = std::sync::mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let _pause = crate::app::query_lifecycle::authority_test_hooks::install_after_root_capture(
            &worker_cassie,
            &worker_session,
            captured_tx,
            resume_rx,
        );
        worker_cassie.execute_sql(&worker_session, "ROLLBACK", vec![])
    });
    captured_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    session.rollback_transaction();
    session.begin_transaction(None).unwrap();
    let owner_b = session.transaction_unique_owner_id();

    // Act
    resume_tx.send(()).unwrap();
    let old = worker.join().unwrap();

    // Assert
    assert_eq!(old.unwrap_err().sql_state(), "25000");
    assert!(session.is_transaction_active());
    assert!(!session.is_transaction_failed());
    assert_eq!(session.transaction_unique_owner_id(), owner_b);
}

#[test]
fn should_fail_the_owned_block_after_a_quarantined_query_error() {
    // Arrange
    let cassie = crate::app::Cassie::new_in_memory_with_config(
        crate::config::CassieRuntimeConfig::default(),
    )
    .unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("quarantine_owner", None);
    session.begin_transaction(None).unwrap();
    cassie.midge.latch_storage_recovery_required_for_test();

    // Act
    let error = cassie
        .execute_sql(&session, "SELECT 1", vec![])
        .unwrap_err();

    // Assert
    assert_eq!(error.sql_state(), "58030");
    assert!(session.is_transaction_failed());
}

#[test]
fn should_not_fail_a_replacement_after_old_quarantine_conversion_fails() {
    // Arrange
    let cassie = crate::app::Cassie::new_in_memory_with_config(
        crate::config::CassieRuntimeConfig::default(),
    )
    .unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("quarantine_converter", None);
    session.begin_transaction(None).unwrap();
    cassie.midge.latch_storage_recovery_required_for_test();
    let worker_cassie = cassie.clone();
    let worker_session = session.clone();
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        worker_cassie.execute_sql_with_cancellation_converted(
            &worker_session,
            "ROLLBACK",
            vec![],
            &crate::runtime::QueryCancellationHandle::new(),
            |_, _| -> Result<(), crate::app::CassieError> {
                entered_tx.send(()).unwrap();
                resume_rx
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                Err(crate::app::CassieError::QueryCanceled)
            },
        )
    });
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    session.begin_transaction(None).unwrap();

    // Act
    resume_tx.send(()).unwrap();
    let result = worker.join().unwrap();

    // Assert
    assert_eq!(result.unwrap_err().sql_state(), "57014");
    assert!(session.is_transaction_active());
    assert!(!session.is_transaction_failed());
}

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
