//! Terminal admission and local transaction cleanup after an unknown write.

use std::time::Instant;

use super::*;
use crate::app::CassieSession;
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn fixture() -> (Cassie, CassieSession) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    (cassie, session)
}

#[test]
fn should_reject_new_constant_queries_after_storage_requires_recovery() {
    // Arrange
    let (cassie, session) = fixture();
    cassie.midge.latch_storage_recovery_required_for_test();

    // Act
    let result = cassie.execute_sql(&session, "SELECT 1", Vec::new());
    let description = cassie.describe_sql("SELECT 1");
    let bind = cassie.validate_pgwire_bind(
        &session,
        crate::sql::parse_statement("SELECT 1").expect("SELECT"),
        0,
        &[],
        &[],
        (
            &QueryCancellationHandle::new(),
            &QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now()),
        ),
    );

    // Assert
    for error in [
        result.expect_err("query unavailable"),
        description.expect_err("describe unavailable"),
        bind.expect_err("bind unavailable"),
    ] {
        assert_eq!(error.descriptor().sql_state, "58030");
    }
}

#[test]
fn should_reject_fresh_native_transaction_acquisition_after_quarantine() {
    // Arrange
    let (cassie, _) = fixture();
    cassie.midge.latch_storage_recovery_required_for_test();

    // Act
    let data = cassie
        .midge
        .data_tx(cntryl_midge::TransactionMode::ReadOnly);
    let schema = cassie
        .midge
        .schema_tx(cntryl_midge::TransactionMode::ReadWrite);
    let temporary = cassie
        .midge
        .temp_tx(cntryl_midge::TransactionMode::ReadWrite);

    // Assert
    for result in [data, schema, temporary] {
        let error = result.err().expect("new native transaction rejected");
        assert_eq!(error.descriptor().sql_state, "58030");
        assert!(error.storage_retry_kind().is_none());
    }
}

#[test]
fn should_release_repeatable_read_ownership_on_quarantined_rollback() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(
            &session,
            "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY",
            Vec::new(),
        )
        .expect("begin");
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    cassie
        .prepare_transaction_read_view(
            &session,
            &crate::sql::parse_statement("SELECT 1").expect("SELECT"),
            &controls,
        )
        .expect("capture");
    assert!(controls.current_query_memory_bytes() > 0);
    cassie.midge.latch_storage_recovery_required_for_test();

    // Act
    let result = cassie
        .execute_sql(&session, "ABORT WORK", Vec::new())
        .expect("local rollback");

    // Assert
    assert_eq!(result.command(), "ROLLBACK");
    assert!(!session.is_transaction_active());
    assert!(!session.is_transaction_failed());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retire_quarantined_commit_without_publishing_staged_rows() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE quarantine_rows (id INT)",
            Vec::new(),
        )
        .expect("table");
    cassie
        .execute_sql(&session, "BEGIN", Vec::new())
        .expect("begin");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO quarantine_rows VALUES (1)",
            Vec::new(),
        )
        .expect("stage");
    cassie.midge.latch_storage_recovery_required_for_test();

    // Act
    let result = cassie.execute_sql(&session, "COMMIT", Vec::new());

    // Assert
    assert_eq!(
        result
            .expect_err("unsubmitted commit")
            .descriptor()
            .sql_state,
        "58030"
    );
    assert!(!session.is_transaction_active());
    assert!(!session.is_transaction_failed());
    assert!(session.transaction_writes().is_empty());
}

#[test]
fn should_reject_new_cursor_fetches_after_storage_requires_recovery() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(&session, "BEGIN", Vec::new())
        .expect("begin");
    cassie
        .execute_sql(
            &session,
            "DECLARE retained_cursor CURSOR FOR SELECT 1",
            Vec::new(),
        )
        .expect("cursor");
    cassie.midge.latch_storage_recovery_required_for_test();

    // Act
    let result = cassie.execute_sql(&session, "FETCH NEXT FROM retained_cursor", Vec::new());

    // Assert
    assert_eq!(
        result
            .expect_err("new fetch rejected")
            .descriptor()
            .sql_state,
        "58030"
    );
}

#[test]
fn should_finish_an_existing_cursor_with_owned_descriptors_after_quarantine() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE pinned_before_quarantine (id INT)",
            Vec::new(),
        )
        .expect("table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO pinned_before_quarantine VALUES (1)",
            Vec::new(),
        )
        .expect("seed");
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let owner = session
        .capture_statement_read(&cassie.midge, &cassie.default_database, &controls)
        .expect("pinned statement");
    let scope = crate::midge::adapter::StatementReadScope::enter(Some(&owner));
    let mut cursor = cassie
        .midge
        .open_row_cursor(
            "pinned_before_quarantine",
            crate::midge::adapter::RowDecode::Full,
        )
        .expect("cursor")
        .expect("available");
    cassie.midge.latch_storage_recovery_required_for_test();

    // Act
    let rows = cursor
        .next_documents(&cassie.midge, 1, &controls)
        .expect("already owned cursor view");
    drop(scope);
    let fresh = cassie.execute_sql(
        &session,
        "SELECT id FROM pinned_before_quarantine",
        Vec::new(),
    );

    // Assert
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].payload["id"], serde_json::json!(1));
    assert_eq!(
        fresh.expect_err("new read denied").descriptor().sql_state,
        "58030"
    );
}

#[test]
fn should_release_quarantined_rollback_owners_before_response_admission() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(&session, "BEGIN", Vec::new())
        .expect("begin");
    session
        .stage_document_write("one", "row".to_owned(), serde_json::json!({"id": 1}))
        .expect("stage");
    cassie.midge.latch_storage_recovery_required_for_test();
    let denied = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 1,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let rollback = crate::sql::parse_statement("ROLLBACK").expect("ROLLBACK");

    // Act
    let result = cassie
        .quarantined_statement_result(&session, &rollback.statement, &denied)
        .expect("quarantined result");

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert!(!session.is_transaction_active());
    assert!(session.transaction_writes().is_empty());
    assert_eq!(denied.current_query_memory_bytes(), 0);
}
