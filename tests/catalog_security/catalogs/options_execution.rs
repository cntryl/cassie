//! Logical option arrays, empty relational behavior and retained execution owners.
use crate::support_options_execution::physical;
use crate::support_sql_fixture::sql_fixture;
use cassie::app::CassieError;
use cassie::runtime::{QueryCancellationHandle, QueryExecutionControls};
use cassie::types::Value;
use std::time::Instant;

#[test]
fn should_expand_stored_arrays_given_a_lateral_source_when_options_are_queried() {
    // Arrange
    let fixture = sql_fixture(
        "options-stored",
        &["CREATE TABLE option_rows(id INT,options TEXT[])"],
    );
    let collection = fixture
        .cassie
        .catalog
        .get_schema("option_rows")
        .expect("array schema")
        .collection;
    fixture
        .cassie
        .ingest_document(&collection, serde_json::json!({"id":1,"options":["a=b"]}))
        .expect("first logical array");
    fixture
        .cassie
        .ingest_document(&collection, serde_json::json!({"id":2,"options":["x=y"]}))
        .expect("second logical array");
    // Act
    let result = fixture.execute("SELECT o.id,p.option_name,p.option_value FROM option_rows o CROSS JOIN LATERAL pg_options_to_table(o.options) AS p ORDER BY o.id").expect("stored options");
    // Assert
    assert_eq!(
        result.rows(),
        &[
            vec![
                Value::Int64(1),
                Value::String("a".into()),
                Value::String("b".into())
            ],
            vec![
                Value::Int64(2),
                Value::String("x".into()),
                Value::String("y".into())
            ]
        ]
    );
}

#[test]
fn should_preserve_left_rows_given_null_options_when_an_outer_join_is_executed() {
    // Arrange
    let fixture = sql_fixture(
        "options-left",
        &[
            "CREATE TABLE option_left(id INT)",
            "INSERT INTO option_left VALUES (1),(2)",
        ],
    );
    // Act
    let result = fixture.execute("SELECT l.id,p.option_name,p.option_value FROM option_left l LEFT JOIN pg_options_to_table(NULL::TEXT[]) p ON TRUE ORDER BY l.id").expect("empty option outer join");
    // Assert
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Int64(1), Value::Null, Value::Null],
            vec![Value::Int64(2), Value::Null, Value::Null]
        ]
    );
}

#[test]
fn should_cancel_expansion_given_cancelled_controls_when_options_are_executed() {
    // Arrange
    let fixture = sql_fixture("options-cancel", &[]);
    let plan = physical(
        &fixture.cassie,
        "SELECT * FROM pg_options_to_table('{a=b}')",
    );
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();
    let controls = QueryExecutionControls::with_cancellation(
        &cassie::config::CassieRuntimeConfig::default().limits,
        Instant::now(),
        cancellation,
    );
    // Act
    let result = cassie::executor::run_with_controls(&fixture.cassie, &plan, vec![], &controls);
    // Assert
    assert!(matches!(
        result.map_err(CassieError::from),
        Err(CassieError::QueryCancelled)
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_denied_expansion_given_a_tiny_budget_when_options_are_executed() {
    // Arrange
    let fixture = sql_fixture("options-budget", &[]);
    let plan = physical(
        &fixture.cassie,
        "SELECT * FROM pg_options_to_table('{a=b}')",
    );
    let mut limits = cassie::config::CassieRuntimeConfig::default().limits;
    limits.query_memory_budget_bytes = 256;
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    // Act
    let result = cassie::executor::run_with_controls(&fixture.cassie, &plan, vec![], &controls);
    // Assert
    assert!(matches!(
        result.map_err(CassieError::from),
        Err(CassieError::ResourceLimit(_))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_expansion_given_a_result_clone_when_the_original_owner_drops() {
    // Arrange
    let fixture = sql_fixture("options-owner", &[]);
    let plan = physical(
        &fixture.cassie,
        "SELECT * FROM pg_options_to_table('{a=b}')",
    );
    let controls = QueryExecutionControls::from_limits(
        &cassie::config::CassieRuntimeConfig::default().limits,
        Instant::now(),
    );
    // Act
    let result = cassie::executor::run_with_controls(&fixture.cassie, &plan, vec![], &controls)
        .expect("owned options");
    let retained = result.clone();
    drop(result);
    // Assert
    assert_eq!(
        retained.rows(),
        &[vec![Value::String("a".into()), Value::String("b".into())]]
    );
    assert!(controls.current_query_memory_bytes() > 0);
    drop(retained);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_expansion_given_a_null_element_when_options_fail() {
    // Arrange
    let fixture = sql_fixture("options-null-release", &[]);
    let plan = physical(
        &fixture.cassie,
        "SELECT * FROM pg_options_to_table(ARRAY['a=b',NULL]::TEXT[])",
    );
    let controls = QueryExecutionControls::from_limits(
        &cassie::config::CassieRuntimeConfig::default().limits,
        Instant::now(),
    );
    // Act
    let result = cassie::executor::run_with_controls(&fixture.cassie, &plan, vec![], &controls);
    // Assert
    assert!(matches!(
        result.map_err(CassieError::from),
        Err(CassieError::NullArrayElement)
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_expanding_cast_scratch_given_a_tiny_budget_when_options_are_executed() {
    // Arrange
    let fixture = sql_fixture("options-character-budget", &[]);
    let plan = physical(
        &fixture.cassie,
        "SELECT * FROM pg_options_to_table(('{a=b}'::CHAR(1000000))::TEXT::TEXT[])",
    );
    let mut limits = cassie::config::CassieRuntimeConfig::default().limits;
    limits.query_memory_budget_bytes = 65536;
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    // Act
    let result = cassie::executor::run_with_controls(&fixture.cassie, &plan, vec![], &controls);
    // Assert
    assert!(matches!(
        result.map_err(CassieError::from),
        Err(CassieError::ResourceLimit(_))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_expanding_stored_cast_given_a_tiny_budget_when_options_are_executed() {
    // Arrange
    let fixture = sql_fixture(
        "options-runtime-character-budget",
        &[
            "CREATE TABLE option_cast_input(body TEXT)",
            "INSERT INTO option_cast_input VALUES ('{a=b}')",
        ],
    );
    let plan = physical(&fixture.cassie,"SELECT p.option_name FROM option_cast_input o CROSS JOIN LATERAL pg_options_to_table((o.body::CHAR(1000000))::TEXT::TEXT[]) p");
    let mut limits = cassie::config::CassieRuntimeConfig::default().limits;
    limits.query_memory_budget_bytes = 65536;
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    // Act
    let result = cassie::executor::run_with_controls(&fixture.cassie, &plan, vec![], &controls);
    // Assert
    assert!(matches!(
        result.map_err(CassieError::from),
        Err(CassieError::ResourceLimit(_))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_expand_stored_arrays_given_an_implicit_lateral_call_when_options_are_queried() {
    // Arrange
    let fixture = sql_fixture(
        "options-implicit-lateral",
        &["CREATE TABLE implicit_options(id INT,options TEXT[])"],
    );
    let collection = fixture
        .cassie
        .catalog
        .get_schema("implicit_options")
        .expect("array schema")
        .collection;
    fixture
        .cassie
        .ingest_document(&collection, serde_json::json!({"id":1,"options":["a=b"]}))
        .expect("logical array");
    // Act
    let result = fixture.execute("SELECT o.id,p.option_name,p.option_value FROM implicit_options o CROSS JOIN pg_options_to_table(o.options) p").expect("implicit lateral options");
    // Assert
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Int64(1),
            Value::String("a".into()),
            Value::String("b".into())
        ]]
    );
}
