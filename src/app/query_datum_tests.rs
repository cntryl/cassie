//! Type/source proof and query ownership through the actual embedded entry point.
use super::query_test_fixture::{controls, Fixture};
use super::{CassieError, ExecutionMode, QueryExecutionControls};
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits, ExecutionResultCacheEnabled};
use crate::types::Value;
use std::time::Instant;

fn fixture() -> Fixture {
    let mut config = CassieRuntimeConfig::from_env().expect("fixture config");
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    Fixture::with_config(config)
}

#[test]
fn should_distinguish_declared_integer_widths_given_equal_carriers_when_datum_sizes_are_queried() {
    // Arrange
    let fixture = fixture();
    // Act
    let result = fixture.cassie().execute_sql(&fixture.session, "SELECT pg_column_size(1::SMALLINT), pg_column_size(1::INTEGER), pg_column_size(1::BIGINT)", vec![]).expect("typed observer");
    // Assert
    assert_eq!(
        result.rows(),
        vec![vec![Value::Int64(2), Value::Int64(4), Value::Int64(8)]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23, 23]
    );
}

#[test]
fn should_observe_fixed_column_casts_given_stored_rows_when_declared_width_is_explicit() {
    // Arrange
    let fixture = fixture();
    fixture
        .cassie()
        .execute_sql(
            &fixture.session,
            "CREATE TABLE datum_rows (a SMALLINT, b INTEGER, c BIGINT)",
            vec![],
        )
        .expect("table");
    fixture
        .cassie()
        .execute_sql(
            &fixture.session,
            "INSERT INTO datum_rows (a,b,c) VALUES (1,1,1)",
            vec![],
        )
        .expect("row");
    // Act
    let result = fixture.cassie().execute_sql(&fixture.session,"SELECT pg_column_size(a::SMALLINT), pg_column_size(b::INTEGER), pg_column_size(c::BIGINT) FROM datum_rows",vec![]).expect("stored fixed types");
    // Assert
    assert_eq!(
        result.rows(),
        vec![vec![Value::Int64(2), Value::Int64(4), Value::Int64(8)]]
    );
}

#[test]
fn should_deny_large_text_given_a_tiny_budget_when_datum_size_is_queried() {
    // Arrange
    let fixture = fixture();
    let controls = controls(1024);
    let sql = format!("SELECT pg_column_size('{}'::TEXT)", "x".repeat(4096));
    // Act
    let result = fixture.cassie().execute_sql_with_controls(
        &fixture.session,
        &sql,
        vec![],
        ExecutionMode::SimpleQuery,
        &controls,
    );
    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_deny_large_bound_text_given_a_tiny_budget_when_only_size_is_returned() {
    // Arrange
    let fixture = fixture();
    let small_controls = controls(256 * 1024);
    let large_controls = controls(256 * 1024);
    // Act
    let small = fixture
        .cassie()
        .execute_sql_with_controls(
            &fixture.session,
            "SELECT pg_column_size($1::TEXT)",
            vec![Value::String("x".into())],
            ExecutionMode::SimpleQuery,
            &small_controls,
        )
        .expect("small input fits the same budget");
    let result = fixture.cassie().execute_sql_with_controls(
        &fixture.session,
        "SELECT pg_column_size($1::TEXT)",
        vec![Value::String("x".repeat(512 * 1024))],
        ExecutionMode::SimpleQuery,
        &large_controls,
    );
    // Assert
    assert_eq!(small.rows(), vec![vec![Value::Int64(5)]]);
    drop(small);
    assert_eq!(small_controls.current_query_memory_bytes(), 0);
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(large_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_owned_size_results_given_clones_when_the_first_result_is_dropped() {
    // Arrange
    let fixture = fixture();
    let controls = controls(8 * 1024 * 1024);
    // Act
    let result = fixture
        .cassie()
        .execute_sql_with_controls(
            &fixture.session,
            "SELECT pg_column_size('foo'::TEXT)",
            vec![],
            ExecutionMode::SimpleQuery,
            &controls,
        )
        .expect("size result");
    let retained = result.clone();
    drop(result);
    // Assert
    assert_eq!(retained.rows(), vec![vec![Value::Int64(7)]]);
    assert!(controls.current_query_memory_bytes() > 0);
    drop(retained);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_size_context_given_precancellation_when_query_is_rejected() {
    // Arrange
    let fixture = fixture();
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();
    // Act
    let result = fixture.cassie().execute_sql_with_controls(
        &fixture.session,
        "SELECT pg_column_size(0::INTEGER)",
        vec![],
        ExecutionMode::SimpleQuery,
        &controls,
    );
    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_expanding_unqualified_layouts_given_large_char_width_when_binding_precedes_evaluation(
) {
    // Arrange
    let fixture = fixture();
    let controls = controls(8 * 1024 * 1024);
    // Act
    let result = fixture.cassie().execute_sql_with_controls(
        &fixture.session,
        "SELECT pg_column_size(CAST('x' AS CHAR(100000000)))",
        vec![],
        ExecutionMode::SimpleQuery,
        &controls,
    );
    // Assert
    assert!(
        matches!(result, Err(CassieError::Unsupported(_))),
        "{result:?}"
    );
    assert!(controls.peak_query_memory_bytes() < 32 * 1024);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_observer_memory_given_a_failing_argument_when_execution_unwinds() {
    // Arrange
    let fixture = fixture();
    let controls = controls(8 * 1024 * 1024);
    // Act
    let result = fixture.cassie().execute_sql_with_controls(
        &fixture.session,
        "SELECT pg_column_size(CAST(1 / 0 AS INTEGER))",
        vec![],
        ExecutionMode::SimpleQuery,
        &controls,
    );
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
