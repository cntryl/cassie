//! Retained snapshot admission, recovery and last-owner resource laws.

use std::time::{Duration, Instant};

use super::*;
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::runtime::QueryCancellationHandle;
use crate::sql::ast::{TransactionIsolation, TransactionModes};

fn fixture() -> (Cassie, CassieSession, ParsedStatement) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    session
        .begin_transaction_with_modes(
            Some(TransactionIsolation::RepeatableRead),
            TransactionModes {
                read_only: Some(true),
                deferrable: Some(false),
            },
        )
        .expect("read-only REPEATABLE READ");
    let parsed = crate::sql::parse_statement("SELECT 1").expect("constant SELECT");
    (cassie, session, parsed)
}

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

#[test]
fn should_leave_repeatable_read_unacquired_after_admission_failure() {
    // Arrange
    let (cassie, session, parsed) = fixture();
    let denied = controls(1);
    let admitted = controls(16 * 1024 * 1024);

    // Act
    let failure = cassie.prepare_transaction_read_view(&session, &parsed, &denied);
    let pending = session.repeatable_read_capture_generation();
    cassie
        .prepare_transaction_read_view(&session, &parsed, &admitted)
        .expect("retry admitted snapshot");

    // Assert
    assert!(matches!(failure, Err(CassieError::ResourceLimit(_))));
    assert!(pending.is_some());
    assert_eq!(denied.current_query_memory_bytes(), 0);
    assert!(session.repeatable_read_capture_generation().is_none());
    assert!(admitted.current_query_memory_bytes() > 0);
    session.rollback_transaction();
    assert_eq!(admitted.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_repeatable_read_charge_until_its_last_shared_owner() {
    // Arrange
    let (cassie, session, parsed) = fixture();
    let controls = controls(16 * 1024 * 1024);
    cassie
        .prepare_transaction_read_view(&session, &parsed, &controls)
        .expect("capture");
    let data = session
        .capture_statement_read(&cassie.midge, &cassie.default_database, &controls)
        .expect("share data owner");
    let catalog = session
        .transaction_catalog_snapshot()
        .expect("catalog owner");
    let charged = controls.current_query_memory_bytes();

    // Act
    session.commit_transaction();
    let held_after_commit = controls.current_query_memory_bytes();
    drop(data);
    let catalog_only = controls.current_query_memory_bytes();
    drop(catalog);

    // Assert
    assert!(charged > held_after_commit);
    assert!(held_after_commit > catalog_only);
    assert!(catalog_only > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reuse_repeatable_read_without_a_second_request_charge() {
    // Arrange
    let (cassie, session, parsed) = fixture();
    let first = controls(16 * 1024 * 1024);
    cassie
        .prepare_transaction_read_view(&session, &parsed, &first)
        .expect("first request");
    let second = controls(1);
    let retained = first.current_query_memory_bytes();

    // Act
    cassie
        .prepare_transaction_read_view(&session, &parsed, &second)
        .expect("retained view needs no second admission");

    // Assert
    assert_eq!(first.current_query_memory_bytes(), retained);
    assert_eq!(second.current_query_memory_bytes(), 0);
    drop(session);
    assert_eq!(first.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_snapshot_admission_after_the_originating_query_retires() {
    // Arrange
    let (cassie, session, parsed) = fixture();
    let first = controls(16 * 1024 * 1024);
    let running = cassie.runtime.begin_running_query();
    cassie
        .prepare_transaction_read_view(&session, &parsed, &first)
        .expect("capture");
    let charged = first.current_query_memory_bytes();
    cassie.runtime.record_query_memory(&first);
    let data = session
        .capture_statement_read(&cassie.midge, &cassie.default_database, &first)
        .expect("shared data owner");
    let catalog = session
        .transaction_catalog_snapshot()
        .expect("shared catalog owner");

    // Act
    drop(running);
    drop(first);
    let idle = cassie.runtime.snapshot();
    let later = controls(1);
    cassie
        .prepare_transaction_read_view(&session, &parsed, &later)
        .expect("later statement shares retained admission");
    let still_retained = cassie.runtime.snapshot();
    session.rollback_transaction();
    let shared = cassie.runtime.snapshot();
    drop(data);
    let catalog_only = cassie.runtime.snapshot();
    drop(catalog);
    let retired = cassie.runtime.snapshot();

    // Assert
    assert!(charged > 0);
    assert_eq!(idle.runtime.running_queries, 0);
    assert_eq!(idle.query.current_accounted_memory_bytes, charged as u64);
    assert_eq!(
        still_retained.query.current_accounted_memory_bytes,
        charged as u64
    );
    assert_eq!(later.current_query_memory_bytes(), 0);
    assert!(shared.query.current_accounted_memory_bytes < charged as u64);
    assert!(
        shared.query.current_accounted_memory_bytes
            > catalog_only.query.current_accounted_memory_bytes
    );
    assert!(catalog_only.query.current_accounted_memory_bytes > 0);
    assert_eq!(retired.query.current_accounted_memory_bytes, 0);
}

#[test]
fn should_preserve_repeatable_read_charge_through_failed_savepoint_recovery() {
    // Arrange
    let (cassie, session, parsed) = fixture();
    session.create_savepoint("before_view").expect("savepoint");
    let controls = controls(16 * 1024 * 1024);
    cassie
        .prepare_transaction_read_view(&session, &parsed, &controls)
        .expect("capture after savepoint");
    let charged = controls.current_query_memory_bytes();

    // Act
    let canceled = QueryCancellationHandle::new();
    canceled.cancel();
    let failure = cassie.execute_sql_with_cancellation(&session, "SELECT 1", Vec::new(), &canceled);
    assert!(matches!(failure, Err(CassieError::QueryCanceled)));
    assert!(session.is_transaction_failed());
    session
        .rollback_to_savepoint("before_view")
        .expect("recover");
    let recovered = controls.current_query_memory_bytes();
    session.rollback_transaction();

    // Assert
    assert!(charged > 0);
    assert_eq!(recovered, charged);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_not_acquire_repeatable_read_after_cancel_or_deadline() {
    // Arrange
    let (cassie, session, parsed) = fixture();
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();
    let canceled = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation,
    );
    let mut expired = controls(16 * 1024 * 1024);
    expired.deadline = Some(
        Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("past"),
    );

    // Act
    let cancel_result = cassie.prepare_transaction_read_view(&session, &parsed, &canceled);
    let deadline_result = cassie.prepare_transaction_read_view(&session, &parsed, &expired);

    // Assert
    assert!(matches!(cancel_result, Err(CassieError::QueryCanceled)));
    assert!(matches!(
        deadline_result,
        Err(CassieError::DeadlineExceeded)
    ));
    assert!(session.repeatable_read_capture_generation().is_some());
    assert_eq!(canceled.current_query_memory_bytes(), 0);
    assert_eq!(expired.current_query_memory_bytes(), 0);
}

mod indexes;

thread_local! {
    static PUBLICATION_WAIT: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const {
        std::cell::RefCell::new(None)
    };
}

pub(super) fn before_publication_lock() {
    let callback = PUBLICATION_WAIT.with(|slot| slot.borrow_mut().take());
    if let Some(callback) = callback {
        callback();
    }
}

#[test]
fn should_cancel_repeatable_read_capture_while_publication_is_blocked() {
    // Arrange
    let (cassie, session, parsed) = fixture();
    let cassie = std::sync::Arc::new(cassie);
    let publication = cassie.catalog_publication.read();
    let worker_cassie = std::sync::Arc::clone(&cassie);
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let observed = controls.clone();
    let (waiting_tx, waiting_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        PUBLICATION_WAIT.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || {
                waiting_tx
                    .send(())
                    .expect("announce publication wait boundary");
                resume_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("resume capture");
            }));
        });
        let result = worker_cassie.prepare_transaction_read_view(&session, &parsed, &controls);
        let pending = session.repeatable_read_capture_generation().is_some();
        done_tx.send(()).expect("finished capture");
        (result, pending)
    });
    waiting_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("capture reached held publication gate");

    // Act
    cancellation.cancel();
    resume_tx.send(()).expect("resume interrupted capture");
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("cancellation retires without publication guard");
    let (result, pending) = worker.join().expect("capture worker");
    drop(publication);

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert!(pending);
    assert_eq!(observed.current_query_memory_bytes(), 0);
}

#[test]
fn should_select_encoding_descriptor_authority_for_each_isolation_scope() {
    // Arrange
    for repeatable in [false, true] {
        let (cassie, reader, _) = fixture();
        if !repeatable {
            reader.rollback_transaction();
        }
        let writer = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &writer,
                "CREATE TABLE rr_descriptor_authority (id INT)",
                Vec::new(),
            )
            .expect("create descriptor");
        let controls = controls(16 * 1024 * 1024);
        let owner = reader
            .capture_statement_read(&cassie.midge, &cassie.default_database, &controls)
            .expect("capture authority");
        cassie
            .execute_sql(
                &writer,
                "ALTER TABLE rr_descriptor_authority ADD COLUMN note TEXT",
                Vec::new(),
            )
            .expect("publish later descriptor");

        // Act
        let scope = crate::midge::adapter::StatementReadScope::enter(Some(&owner));
        let schema = cassie
            .midge
            .row_schema("rr_descriptor_authority")
            .expect("selected encoding descriptor");
        drop(scope);

        // Assert
        assert_eq!(
            schema.fields.iter().any(|field| field.name == "note"),
            repeatable
        );
        assert!(cassie
            .catalog
            .get_schema("rr_descriptor_authority")
            .expect("current binding descriptor")
            .fields
            .iter()
            .any(|field| field.name == "note"));
    }
}
