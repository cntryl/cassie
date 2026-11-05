//! Resource and authority probes for a retained Bind view.

use super::*;
use crate::app::Cassie;
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use std::time::Instant;

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn fixture() -> (Cassie, CassieSession) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE bound_rows (id INT PRIMARY KEY, payload TEXT)",
            Vec::new(),
        )
        .expect("table");
    cassie
        .execute_sql(
            &session,
            "BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE",
            Vec::new(),
        )
        .expect("begin");
    cassie
        .execute_sql(&session, "SELECT 1", Vec::new())
        .expect("initial view");
    (cassie, session)
}

#[test]
fn should_avoid_copying_own_payload_for_a_data_free_bind() {
    // Arrange
    let (cassie, session) = fixture();
    session
        .stage_document_write(
            "bound_rows",
            "1".into(),
            serde_json::json!({"id":1,"payload":"x".repeat(8192)}),
        )
        .expect("large own row");
    let header = size_of::<BoundReadView>()
        + 4 * size_of::<usize>()
        + size_of::<Mutex<SessionTransactionState>>();
    let current = controls(header);
    let retention = controls(header);

    // Act
    let frame = session
        .capture_bound_read_view(
            &cassie.midge,
            &cassie.default_database,
            false,
            &current,
            &retention,
        )
        .expect("constant plan fits")
        .expect("RR frame");
    let copied = session.capture_bound_read_view(
        &cassie.midge,
        &cassie.default_database,
        true,
        &controls(header),
        &controls(header),
    );
    let held = current.current_query_memory_bytes();
    drop(frame);

    // Assert
    assert!(matches!(copied, Err(CassieError::ResourceLimit(_))));
    assert_eq!(held, header);
    assert_eq!(current.current_query_memory_bytes(), 0);
    assert_eq!(retention.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_the_connection_quota_until_the_last_frame_owner() {
    // Arrange
    let (cassie, session) = fixture();
    let current = controls(1024 * 1024);
    let retention = controls(1024 * 1024);
    let frame = session
        .capture_bound_read_view(
            &cassie.midge,
            &cassie.default_database,
            false,
            &current,
            &retention,
        )
        .expect("admitted")
        .expect("RR frame");
    let worker = Arc::clone(&frame);
    let charged = retention.current_query_memory_bytes();

    // Act
    drop(frame);
    session.rollback_transaction();
    let worker_charge = retention.current_query_memory_bytes();
    let scope = BoundReadScope::enter(Some(worker));
    let stale = BoundReadScope::take(&session, &cassie.midge, &cassie.default_database);
    drop(scope);

    // Assert
    assert!(charged > 0);
    assert_eq!(worker_charge, charged);
    assert!(matches!(stale, Err(CassieError::Transaction { .. })));
    assert_eq!(retention.current_query_memory_bytes(), 0);
    assert_eq!(current.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_execute_authority_independent_of_the_bind_request() {
    // Arrange
    let (cassie, session) = fixture();
    let old_cancel = crate::runtime::QueryCancellationHandle::new();
    let bind = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        old_cancel.clone(),
    );
    let retention = controls(1024 * 1024);
    let frame = session
        .capture_bound_read_view(
            &cassie.midge,
            &cassie.default_database,
            true,
            &bind,
            &retention,
        )
        .expect("admitted")
        .expect("RR frame");
    old_cancel.cancel();
    let current_cancel = crate::runtime::QueryCancellationHandle::new();
    let fresh = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        current_cancel.clone(),
    );

    // Act
    let _scope = BoundReadScope::enter(Some(frame));
    let owner = BoundReadScope::take(&session, &cassie.midge, &cassie.default_database)
        .expect("old cancellation not retained")
        .expect("view");
    let execute = fresh.with_statement_read(owner.data().cloned());
    let admitted = execute.check_interruption();
    current_cancel.cancel();
    let cancelled = execute.check_interruption();

    // Assert
    assert!(admitted.is_ok());
    assert!(matches!(cancelled, Err(CassieError::QueryCancelled)));
}

#[test]
fn should_reuse_the_read_only_transaction_without_a_bind_frame() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY",
            Vec::new(),
        )
        .expect("begin");
    cassie
        .execute_sql(&session, "SELECT 1", Vec::new())
        .expect("initial owner");
    let current = controls(1);
    let retention = controls(1);
    let original = session
        .repeatable_committed_read(&cassie.midge, &cassie.default_database)
        .expect("data")
        .expect("retained");

    // Act
    let frame = session
        .capture_bound_read_view(
            &cassie.midge,
            &cassie.default_database,
            true,
            &current,
            &retention,
        )
        .expect("no additional backing");
    let reused = session
        .repeatable_committed_read(&cassie.midge, &cassie.default_database)
        .expect("reuse")
        .expect("retained");

    // Assert
    assert!(frame.is_none());
    assert!(Arc::ptr_eq(&original, &reused));
    assert_eq!(current.current_query_memory_bytes(), 0);
    assert_eq!(retention.current_query_memory_bytes(), 0);
}

#[test]
fn should_reserve_native_write_authority_for_restorable_modes() {
    // Arrange
    let cases: &[(&str, &[&str], &[&str], bool)] = &[
        ("READ WRITE", &[], &[], true),
        ("READ WRITE", &[], &["SET transaction_read_only = on"], true),
        (
            "READ WRITE",
            &["SET transaction_read_only = on"],
            &[],
            false,
        ),
        ("READ ONLY", &[], &[], false),
        ("READ ONLY", &["SET transaction_read_only = off"], &[], true),
        (
            "READ WRITE",
            &["SAVEPOINT writable", "SET transaction_read_only = on"],
            &[],
            true,
        ),
        (
            "READ WRITE",
            &["SAVEPOINT writable", "SET transaction_read_only = on"],
            &["ROLLBACK TO SAVEPOINT writable"],
            true,
        ),
        (
            "READ WRITE",
            &[
                "SAVEPOINT writable",
                "SET transaction_read_only = on",
                "SAVEPOINT readonly",
            ],
            &["RELEASE SAVEPOINT writable"],
            true,
        ),
    ];
    for (mode, before, after, retained_rw) in cases {
        let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
            .expect("memory fixture");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                &format!("BEGIN ISOLATION LEVEL REPEATABLE READ {mode}"),
                Vec::new(),
            )
            .expect("begin");
        for sql in *before {
            cassie
                .execute_sql(&session, sql, Vec::new())
                .expect("mode before first snapshot");
        }

        // Act
        cassie
            .execute_sql(&session, "SELECT 1", Vec::new())
            .expect("capture");
        for sql in *after {
            cassie
                .execute_sql(&session, sql, Vec::new())
                .expect("mode after capture");
        }
        let native = cassie
            .midge
            .data_tx(cntryl_midge::TransactionMode::ReadWrite);
        let blocked = match native {
            Ok(transaction) => {
                transaction.rollback().expect("retire external capability");
                false
            }
            Err(CassieError::Unsupported(_)) => true,
            Err(error) => panic!("unexpected native admission: {error}"),
        };
        session.rollback_transaction();

        // Assert
        assert_eq!(blocked, *retained_rw, "{mode}; {before:?}; {after:?}");
    }
}
