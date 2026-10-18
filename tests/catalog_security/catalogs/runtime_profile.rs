//! Runtime metadata reports Cassie's actual profile and SQL value representation.
use crate::support_sql_fixture::sql_fixture;
use cassie::app::CassieError;
use cassie::runtime::{QueryCancellationHandle, QueryExecutionControls};
use cassie::types::Value;

#[test]
fn should_report_actual_unicode_tables_given_the_rust_runtime_when_unicode_version_is_selected() {
    // Arrange
    let fixture = sql_fixture("runtime-unicode-profile", &[]);
    let (major, minor, patch) = std::char::UNICODE_VERSION;
    // Act
    let result = fixture
        .execute("SELECT unicode_version() AS version")
        .expect("Unicode profile");
    // Assert
    assert_eq!(result.columns()[0].type_oid, 25);
    assert_eq!(
        result.rows(),
        &[vec![Value::String(format!("{major}.{minor}.{patch}"))]]
    );
}

#[test]
fn should_report_uncompressed_values_given_stored_text_when_compression_is_selected() {
    // Arrange
    let fixture = sql_fixture(
        "runtime-stored-compression",
        &[
            "CREATE TABLE runtime_text(id INT, body TEXT)",
            "INSERT INTO runtime_text VALUES (1,'plain'),(2,NULL)",
        ],
    );
    // Act
    let result = fixture
        .execute("SELECT pg_column_compression(body) AS compression FROM runtime_text ORDER BY id")
        .expect("materialized SQL values");
    // Assert
    assert_eq!(result.columns()[0].type_oid, 25);
    assert_eq!(result.rows(), &[vec![Value::Null], vec![Value::Null]]);
}

#[test]
fn should_count_zero_modules_given_no_native_plugin_loader_when_loaded_modules_is_aggregated() {
    // Arrange
    let fixture = sql_fixture("runtime-module-count", &[]);
    // Act
    let result = fixture
        .execute("SELECT COUNT(*) FROM pg_get_loaded_modules()")
        .expect("module count");
    // Assert
    assert_eq!(result.rows(), &[vec![Value::Int64(0)]]);
}

#[test]
fn should_preserve_left_rows_given_an_empty_module_set_when_a_left_join_is_executed() {
    // Arrange
    let fixture = sql_fixture(
        "runtime-module-left-join",
        &[
            "CREATE TABLE runtime_left(id INT)",
            "INSERT INTO runtime_left VALUES (1),(2)",
        ],
    );
    // Act
    let result = fixture.execute("SELECT l.id,m.module_name FROM runtime_left l LEFT JOIN pg_get_loaded_modules() m ON TRUE ORDER BY l.id").expect("empty module outer join");
    // Assert
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Int64(1), Value::Null],
            vec![Value::Int64(2), Value::Null]
        ]
    );
}

#[test]
fn should_cancel_module_discovery_given_cancelled_controls_when_the_table_function_is_executed() {
    // Arrange
    let fixture = sql_fixture("runtime-module-cancel", &[]);
    let parsed =
        cassie::sql::parse_statement("SELECT * FROM pg_get_loaded_modules()").expect("parse");
    let bound = cassie::sql::binder::bind(parsed, &fixture.cassie.catalog).expect("bind");
    let logical = cassie::planner::logical::plan(&bound).expect("plan");
    let plan = std::sync::Arc::new(cassie::planner::physical::build(logical));
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();
    let controls = QueryExecutionControls::with_cancellation(
        &cassie::config::CassieRuntimeConfig::default().limits,
        std::time::Instant::now(),
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
fn should_evaluate_compression_input_given_an_invalid_cast_when_a_stored_value_is_selected() {
    // Arrange
    let fixture = sql_fixture(
        "runtime-compression-evaluation",
        &[
            "CREATE TABLE runtime_bad(body TEXT)",
            "INSERT INTO runtime_bad VALUES ('invalid')",
        ],
    );
    // Act
    let result =
        fixture.execute("SELECT pg_column_compression(CAST(body AS INT)) FROM runtime_bad");
    // Assert
    let error = result.expect_err("compression must evaluate its argument");
    assert!(error.to_string().contains("invalid"), "{error}");
}
