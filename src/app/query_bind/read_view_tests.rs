//! The actual Bind plan proof excludes large own maps for constant queries.

use super::*;
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};

#[test]
fn should_prove_a_constant_bind_does_not_require_statement_data() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE bind_budget_rows (id INT PRIMARY KEY, payload TEXT)",
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
        .expect("first snapshot");
    let budget = 64 * 1024;
    session
        .stage_document_write(
            "bind_budget_rows",
            "1".into(),
            serde_json::json!({"id":1,"payload":"x".repeat(budget)}),
        )
        .expect("own row larger than available copy budget");
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: budget,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let retention = QueryExecutionControls::from_limits(&limits, Instant::now());
    let parsed = crate::sql::parse_statement("SELECT 1").expect("constant");
    let fingerprint = crate::runtime::sql_fingerprint(&parsed);

    // Act
    let needs_data = cassie
        .validate_pgwire_bind_with_controls(&session, parsed, fingerprint, &[], &[], &controls)
        .expect("actual Bind validation")
        .expect("query read shape");
    let owner = session
        .capture_bound_read_view(
            &cassie.midge,
            &cassie.default_database,
            needs_data,
            &controls,
            &retention,
        )
        .expect("bound view fits")
        .expect("RR frame");
    let data_capture = session.capture_statement_read(
        &cassie.midge,
        &cassie.default_database,
        &QueryExecutionControls::from_limits(&limits, Instant::now()),
    );
    drop(owner);

    // Assert
    assert!(!needs_data);
    assert!(matches!(data_capture, Err(CassieError::ResourceLimit(_))));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(retention.current_query_memory_bytes(), 0);
}
