//! Measured owner charge and materialized-source overlap for portal calibration.
use super::{direct_plan, execute, Cassie};
use crate::runtime::QueryExecutionControls;
use std::sync::Arc;

#[test]
fn should_measure_combined_statement_owner_charge_during_materialized_source_execution() {
    // Arrange
    let path = std::env::temp_dir().join(format!("cassie-owner-overlap-{}", uuid::Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("reader", None);
    execute(
        &cassie,
        &session,
        "CREATE TABLE portal_shared_memory (payload TEXT)",
    );
    for index in 0..64 {
        cassie
            .midge
            .put_document(
                "portal_shared_memory",
                Some(format!("doc-{index:04}")),
                serde_json::json!({"payload":format!("{index:04}-{}", "x".repeat(1024))}),
            )
            .expect("seed exact portal fixture");
    }
    execute(&cassie, &session, "BEGIN");
    let plan = direct_plan(&cassie, "SELECT lower(payload) AS payload FROM portal_shared_memory WHERE payload IS NOT NULL LIMIT 1001 OFFSET 0");
    let mut limits = cassie.runtime.limits();
    limits.query_memory_budget_bytes = 258 * 1024;
    let data_controls = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
    let data = crate::midge::adapter::StatementDataRead::capture(
        &cassie.midge,
        &cassie.default_database,
        &data_controls,
    )
    .expect("Data-only capture");
    let data_charge = data_controls.current_query_memory_bytes();
    drop(data);
    assert_eq!(data_controls.current_query_memory_bytes(), 0);
    let controls = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
    let owner = session
        .capture_statement_read(&cassie.midge, &cassie.default_database, &controls)
        .expect("combined capture");
    let owner_charge = controls.current_query_memory_bytes();
    let captured = controls.with_statement_read(Some(Arc::clone(&owner)));
    let references_before = Arc::strong_count(&owner);
    // Act
    let result = crate::executor::run_with_session_controls(
        &cassie,
        Some(&session),
        &plan,
        vec![],
        &captured,
    )
    .expect("materialized source at calibrated budget");
    let peak = controls.peak_query_memory_bytes();
    let references_after = Arc::strong_count(&owner);
    let rows = result.rows.len();
    drop(result);
    let retained = controls.current_query_memory_bytes();
    drop(captured);
    drop(owner);
    let released = controls.current_query_memory_bytes();
    // Assert
    eprintln!("owner overlap data_only={data_charge} overlay={} combined={owner_charge} source_execution_peak={peak} retained={retained} released={released} refs_before={references_before} refs_after={references_after} rows={rows}", owner_charge-data_charge);
    assert!(owner_charge > data_charge);
    assert_eq!(rows, 64);
    assert!(peak > owner_charge);
    assert!(peak <= limits.query_memory_budget_bytes);
    assert_eq!(references_before, 2);
    assert_eq!(references_after, references_before);
    assert_eq!(retained, owner_charge);
    assert_eq!(released, 0);
    drop(session);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict fixture cleanup");
}

#[test]
fn should_release_automatically_captured_owners_after_materialized_source_denial() {
    // Arrange
    let path = std::env::temp_dir().join(format!("cassie-owner-denial-{}", uuid::Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("reader", None);
    execute(
        &cassie,
        &session,
        "CREATE TABLE denied_source (payload TEXT)",
    );
    for index in 0..64 {
        cassie
            .midge
            .put_document(
                "denied_source",
                Some(format!("doc-{index:04}")),
                serde_json::json!({"payload":format!("{index:04}-{}", "x".repeat(1024))}),
            )
            .expect("seed exact source fixture");
    }
    execute(&cassie, &session, "BEGIN");
    let plan = direct_plan(&cassie, "SELECT lower(payload) AS payload FROM denied_source WHERE payload IS NOT NULL LIMIT 1001 OFFSET 0");
    let mut limits = cassie.runtime.limits();
    limits.query_memory_budget_bytes = 256 * 1024;
    let controls = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
    // Act
    let denied = crate::executor::run_with_session_controls(
        &cassie,
        Some(&session),
        &plan,
        vec![],
        &controls,
    )
    .expect_err("original budget denies source overlap");
    let peak = controls.peak_query_memory_bytes();
    let released = controls.current_query_memory_bytes();
    limits.query_memory_budget_bytes = 258 * 1024;
    let retry_controls = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
    let retry = crate::executor::run_with_session_controls(
        &cassie,
        Some(&session),
        &plan,
        vec![],
        &retry_controls,
    )
    .expect("fresh admitted source retry");
    let rows = retry.rows.len();
    drop(retry);
    // Assert
    eprintln!("source denial={denied} successful_reservation_peak_before_denial={peak} released={released} retry_rows={rows} retry_release={}", retry_controls.current_query_memory_bytes());
    assert!(denied.to_string().contains("memory budget exceeded"));
    assert!(peak > 0);
    assert_eq!(released, 0);
    assert_eq!(rows, 64);
    assert_eq!(retry_controls.current_query_memory_bytes(), 0);
    assert_eq!(cassie.runtime.snapshot().runtime.active_operator_workers, 0);
    drop(session);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict fixture cleanup");
}
