//! An outer wire completion uses its admitted operation, never the next block.

use super::{CassieSession, FailureAuthority};
use std::cell::RefCell;
use std::sync::mpsc::{Receiver, SyncSender};
type SessionObserver = Box<dyn FnOnce(&CassieSession)>;

#[test]
fn should_refuse_old_savepoint_commands_before_mutating_a_same_named_replacement() {
    for sql in [
        "SAVEPOINT s",
        "ROLLBACK TO SAVEPOINT s",
        "RELEASE SAVEPOINT s",
        "BEGIN",
    ] {
        // Arrange
        let cassie = crate::app::Cassie::new_in_memory_with_config(
            crate::config::CassieRuntimeConfig::default(),
        )
        .unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("savepoint_owner", None);
        session.begin_transaction(None).unwrap();
        session.create_savepoint("s").unwrap();
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
            worker_cassie.execute_sql(&worker_session, sql, vec![])
        });
        captured_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        session.rollback_transaction();
        if sql == "BEGIN" {
            session.begin_implicit_transaction().unwrap();
        } else {
            session.begin_transaction(None).unwrap();
            session.create_savepoint("s").unwrap();
        }
        let owner_b = session.transaction_unique_owner_id();
        let epoch_b = session.rollback_epoch_for_test();
        let marks_b = session.transaction.lock().savepoints.len();

        // Act
        resume_tx.send(()).unwrap();
        let old = worker.join().unwrap();

        // Assert
        assert_eq!(old.unwrap_err().sql_state(), "25000", "stale command {sql}");
        assert_eq!(session.transaction_unique_owner_id(), owner_b);
        assert_eq!(session.rollback_epoch_for_test(), epoch_b);
        assert_eq!(session.transaction.lock().savepoints.len(), marks_b);
        assert!(session.is_transaction_active());
        assert!(!session.is_transaction_failed());
        if sql == "BEGIN" {
            assert!(session.is_implicit_transaction());
        }
    }
}

#[test]
fn should_refuse_old_sql_setting_leaves_without_changing_the_replacement() {
    for sql in [
        "SET application_name = 'old'",
        "RESET application_name",
        "RESET ALL",
        "SET search_path TO old_schema",
        "RESET search_path",
    ] {
        // Arrange
        let cassie = crate::app::Cassie::new_in_memory_with_config(
            crate::config::CassieRuntimeConfig::default(),
        )
        .unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("setting_owner", None);
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
            worker_cassie.execute_sql(&worker_session, sql, vec![])
        });
        captured_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        session.rollback_transaction();
        session.begin_transaction(None).unwrap();
        session
            .set_setting("application_name", "replacement")
            .unwrap();
        session.set_search_path(vec!["replacement_schema".into()]);

        // Act
        resume_tx.send(()).unwrap();
        let result = worker.join().unwrap();

        // Assert
        assert_eq!(
            result.unwrap_err().sql_state(),
            "25000",
            "old setting {sql}"
        );
        assert_eq!(session.setting("application_name").unwrap(), "replacement");
        assert_eq!(session.search_path(), ["replacement_schema"]);
        assert!(session.is_transaction_active());
        assert!(!session.is_transaction_failed());
    }
}

#[test]
fn should_reject_late_root_bookkeeping_and_staging_without_changing_the_replacement() {
    // Arrange
    let cassie = crate::app::Cassie::new_in_memory_with_config(
        crate::config::CassieRuntimeConfig::default(),
    )
    .unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("root_bookkeeping", None);
    session.begin_transaction(None).unwrap();
    let authority = FailureAuthority::capture(&session);
    let operation = session.with_operation_failure_authority(Some(&authority));
    session.rollback_transaction();
    session.begin_transaction(None).unwrap();
    let controls = crate::runtime::QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        std::time::Instant::now(),
    );

    // Act
    operation.mark_transaction_query();
    let write = operation.stage_document_write(
        "postgres.public.guard_rows",
        "old".into(),
        serde_json::json!({"id": 7}),
    );
    let local = operation.set_local_setting("statement_timeout", "1");
    let isolation = operation.set_transaction_isolation("repeatable read");
    let catalog = operation.begin_transaction_catalog_overlay(&cassie.catalog, &controls);
    let overlay = operation.transaction_storage_overlay();

    // Assert
    assert_eq!(write.unwrap_err().sql_state(), "25000");
    assert_eq!(local.unwrap_err().sql_state(), "25000");
    assert_eq!(isolation.unwrap_err().sql_state(), "25000");
    assert_eq!(catalog.unwrap_err().sql_state(), "25000");
    assert_eq!(overlay.unwrap_err().sql_state(), "25000");
    let transaction = session.transaction.lock();
    assert!(!transaction.has_query);
    assert!(transaction.writes.is_empty());
    assert!(transaction.local_settings.is_empty());
    assert!(transaction.catalog_overlay.is_none());
    assert_eq!(transaction.isolation, None);
}

#[test]
fn should_not_export_a_replacements_mutable_catalog_alias_to_an_old_operation() {
    // Arrange
    let cassie = crate::app::Cassie::new_in_memory_with_config(
        crate::config::CassieRuntimeConfig::default(),
    )
    .unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("catalog_alias_owner", None);
    session.begin_transaction(None).unwrap();
    let authority = FailureAuthority::capture(&session);
    let operation = session.with_operation_failure_authority(Some(&authority));
    session.rollback_transaction();
    session.begin_transaction(None).unwrap();
    let controls = crate::runtime::QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        std::time::Instant::now(),
    );
    session
        .begin_transaction_catalog_overlay(&cassie.catalog, &controls)
        .unwrap();

    // Act
    let stale_alias = operation.transaction_catalog_overlay();

    // Assert
    assert!(stale_alias.is_none());
    assert!(session.transaction_catalog_overlay().is_some());
}

thread_local! {
    static OWN_START: RefCell<Option<(SyncSender<()>, Receiver<()>)>> = const { RefCell::new(None) };
    static SAVEPOINT_CLEANUP: RefCell<Option<SessionObserver>> = const { RefCell::new(None) };
    static LOCAL_SETTING_WRITE: RefCell<Option<SessionObserver>> = const { RefCell::new(None) };
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
    pub(crate) fn observe_local_setting_write_for_test(&self) {
        if let Some(observer) = LOCAL_SETTING_WRITE.with(|slot| slot.borrow_mut().take()) {
            observer(self);
        }
    }
    pub(crate) fn observe_savepoint_cleanup_for_test(&self) {
        if let Some(observer) = SAVEPOINT_CLEANUP.with(|slot| slot.borrow_mut().take()) {
            observer(self);
        }
    }
    pub(crate) fn pause_owned_transaction_start_for_test(
        entered: SyncSender<()>,
        resume: Receiver<()>,
    ) -> OwnedStartPause {
        OWN_START.with(|slot| assert!(slot.replace(Some((entered, resume))).is_none()));
        OwnedStartPause
    }

    pub(crate) fn observe_owned_transaction_start_for_test() {
        if let Some((entered, resume)) = OWN_START.with(|slot| slot.borrow_mut().take()) {
            entered.send(()).unwrap();
            resume
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
        }
    }
}

#[test]
fn should_keep_local_setting_history_and_the_actual_write_under_one_root_lock() {
    // Arrange
    let session = CassieSession::new("local_setting_owner".into(), None);
    session.begin_transaction(None).unwrap();
    let observed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observer = std::sync::Arc::clone(&observed);
    LOCAL_SETTING_WRITE.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move |session| {
            assert!(session.transaction.try_lock().is_none());
            assert_eq!(
                session
                    .settings
                    .lock()
                    .get_ref("statement_timeout")
                    .unwrap(),
                "0"
            );
            observer.store(true, std::sync::atomic::Ordering::Release);
        }))
    });

    // Act
    session.set_local_setting("statement_timeout", "1").unwrap();
    let changed = session
        .settings
        .lock()
        .get_ref("statement_timeout")
        .unwrap()
        .to_owned();
    session.rollback_transaction();

    // Assert
    assert!(observed.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(changed, "1ms");
    assert_eq!(
        session
            .settings
            .lock()
            .get_ref("statement_timeout")
            .unwrap(),
        "0"
    );
}

#[test]
fn should_restore_savepoint_settings_and_objects_before_releasing_the_root_mutex() {
    // Arrange
    let session = CassieSession::new("savepoint_cleanup".into(), None);
    session.begin_transaction(None).unwrap();
    session.create_savepoint("s").unwrap();
    let before = session
        .settings
        .lock()
        .get_ref("statement_timeout")
        .unwrap()
        .to_owned();
    session.set_local_setting("statement_timeout", "1").unwrap();
    let observed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observer = std::sync::Arc::clone(&observed);
    SAVEPOINT_CLEANUP.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move |session| {
            assert!(session.transaction.try_lock().is_none());
            assert_eq!(
                session
                    .settings
                    .lock()
                    .get_ref("statement_timeout")
                    .unwrap(),
                before
            );
            observer.store(true, std::sync::atomic::Ordering::Release);
        }))
    });

    // Act
    session.rollback_to_savepoint("s").unwrap();

    // Assert
    assert!(observed.load(std::sync::atomic::Ordering::Acquire));
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
fn should_preserve_the_captured_owner_when_recovery_latches_after_admission() {
    for route in [0, 1, 2] {
        // Arrange
        let cassie = crate::app::Cassie::new_in_memory_with_config(
            crate::config::CassieRuntimeConfig::default(),
        )
        .unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("quarantine_route", None);
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
            let cancellation = crate::runtime::QueryCancellationHandle::new();
            match route {
                0 => worker_cassie.execute_sql(&worker_session, "ROLLBACK", vec![]),
                1 => worker_cassie.execute_pgwire_sql_with_cancellation(
                    &worker_session,
                    ("ROLLBACK", "ROLLBACK"),
                    &cancellation,
                ),
                _ => {
                    let parsed = crate::sql::parse_statement("ROLLBACK").unwrap();
                    let fingerprint = crate::runtime::sql_fingerprint(&parsed);
                    worker_cassie.execute_parsed_sql_with_cancellation(
                        &worker_session,
                        (parsed, "ROLLBACK"),
                        fingerprint,
                        (vec![], &[]),
                        crate::runtime::ExecutionMode::ExtendedQuery,
                        &cancellation,
                    )
                }
            }
        });
        captured_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        session.rollback_transaction();
        session.begin_transaction(None).unwrap();
        let owner_b = session.transaction_unique_owner_id();
        cassie.midge.latch_storage_recovery_required_for_test();

        // Act
        resume_tx.send(()).unwrap();
        let result = worker.join().unwrap();

        // Assert
        assert_eq!(
            result.unwrap_err().sql_state(),
            "25000",
            "quarantine route {route}"
        );
        assert!(session.is_transaction_active());
        assert!(!session.is_transaction_failed());
        assert_eq!(session.transaction_unique_owner_id(), owner_b);
        assert_eq!(
            cassie
                .execute_sql(&session, "SELECT 1", vec![])
                .unwrap_err()
                .sql_state(),
            "58030",
            "late stale rejection must not clear the storage recovery fence"
        );
    }
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

#[test]
fn should_keep_an_established_owner_when_later_admission_is_rejected() {
    // Arrange
    let session = CassieSession::new("owned_rejection".into(), None);
    let operation = session.with_new_wire_operation();
    operation
        .begin_implicit_transaction_for_operation(operation.operation_failure_authority())
        .unwrap();
    let owner = operation.operation_failure_authority().unwrap();
    let receipt = owner.portal_adoption();

    // Act
    operation.reject_operation_admission();
    let retained = owner.portal_adoption();
    operation.fail_operation_transaction();

    // Assert
    assert!(receipt.is_some());
    assert_eq!(retained, receipt);
    assert!(!operation.operation_was_rejected());
    assert!(!session.is_transaction_active());
}

#[test]
fn should_not_grant_an_established_rejected_operation_rights_over_a_replacement() {
    // Arrange
    let session = CassieSession::new("replaced_rejection".into(), None);
    session.begin_transaction(None).unwrap();
    let operation = session.with_new_wire_operation();
    let original = FailureAuthority::capture_for_operation(&operation).identity();
    session.rollback_transaction();
    session.begin_transaction(None).unwrap();

    // Act
    operation.reject_operation_admission();
    operation.fail_operation_transaction();

    // Assert
    assert_eq!(
        operation.operation_failure_authority().unwrap().identity(),
        original
    );
    assert!(session.is_transaction_active());
    assert!(!session.is_transaction_failed());
}

#[test]
fn should_not_export_a_replacements_repeatable_read_alias_to_an_old_operation() {
    // Arrange
    let cassie = crate::app::Cassie::new_in_memory_with_config(
        crate::config::CassieRuntimeConfig::default(),
    )
    .unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("repeatable_alias", None);
    cassie
        .execute_sql(&session, "CREATE TABLE rr_alias_rows (id INT)", vec![])
        .unwrap();
    cassie
        .execute_sql(&session, "BEGIN ISOLATION LEVEL REPEATABLE READ", vec![])
        .unwrap();
    cassie
        .execute_sql(&session, "SELECT id FROM rr_alias_rows", vec![])
        .unwrap();
    let operation = session.with_new_wire_operation();
    FailureAuthority::capture_for_operation(&operation);
    session.rollback_transaction();
    cassie
        .execute_sql(&session, "BEGIN ISOLATION LEVEL REPEATABLE READ", vec![])
        .unwrap();
    cassie
        .execute_sql(&session, "SELECT id FROM rr_alias_rows", vec![])
        .unwrap();
    let controls = cassie.runtime.query_controls(std::time::Instant::now());
    let current = session.repeatable_read_identity();

    // Act
    let statement =
        operation.repeatable_statement_read(&cassie.midge, &cassie.default_database, &controls);
    let committed = operation.repeatable_committed_read(&cassie.midge, &cassie.default_database);
    let live = session
        .repeatable_committed_read(&cassie.midge, &cassie.default_database)
        .unwrap();

    // Assert
    for error in [statement.unwrap_err(), committed.unwrap_err()] {
        assert!(matches!(
            error,
            crate::app::CassieError::Transaction {
                kind: crate::app::TransactionErrorKind::InvalidState,
                ..
            }
        ));
    }
    assert!(live.is_some());
    assert_eq!(session.repeatable_read_identity(), current);
    assert!(!session.is_transaction_failed());
}

#[test]
fn should_detach_only_the_explicit_unscoped_cleanup_clone() {
    // Arrange
    let root = CassieSession::new("cleanup_clone".into(), None);
    root.begin_transaction(None).unwrap();
    let operation = root.with_new_wire_operation();
    let owner = FailureAuthority::capture_for_operation(&operation).identity();

    // Act
    let cleanup = operation.without_operation_failure_authority();
    cleanup.rollback_transaction();
    cleanup.begin_transaction(None).unwrap();

    // Assert
    assert!(cleanup.operation_failure_authority().is_none());
    assert_eq!(
        operation.operation_failure_authority().unwrap().identity(),
        owner
    );
    assert!(operation
        .ensure_admitted_operation_owner(operation.operation_failure_authority().unwrap())
        .is_err());
    assert!(!root.is_transaction_failed());
}
