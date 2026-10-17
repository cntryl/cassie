//! Size and encoding helpers on real stored rows and reusable plans.
use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::CassieError;
use cassie::runtime::{QueryCancellationHandle, QueryExecutionControls};
use cassie::types::Value;

#[test]
fn should_parse_stored_sizes_given_decimal_text_when_rows_are_projected() {
    // Arrange
    let fixture = sql_fixture(
        "catalog-size-stored",
        &[
            "CREATE TABLE scalar_sizes(id INT, value TEXT)",
            "INSERT INTO scalar_sizes VALUES (1,'1.5 MB'),(2,'-0.5 B'),(3,NULL)",
        ],
    );
    // Act
    let result = fixture
        .execute("SELECT pg_size_bytes(value) AS output FROM scalar_sizes ORDER BY id")
        .expect("stored size parsing");
    // Assert
    assert_eq!(result.columns()[0].type_oid, 20);
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Int64(1_572_864)],
            vec![Value::Int64(-1)],
            vec![Value::Null]
        ]
    );
}

#[test]
fn should_format_stored_sizes_given_bigint_values_when_rows_are_projected() {
    // Arrange
    let fixture = sql_fixture(
        "catalog-pretty-stored",
        &[
            "CREATE TABLE scalar_sizes(id INT, value BIGINT)",
            "INSERT INTO scalar_sizes VALUES (1,10752),(2,-10752),(3,NULL)",
        ],
    );
    // Act
    let result = fixture
        .execute("SELECT pg_size_pretty(value) AS output FROM scalar_sizes ORDER BY id")
        .expect("stored size formatting");
    // Assert
    assert_eq!(result.columns()[0].type_oid, 25);
    assert_eq!(
        result.rows(),
        &[
            vec![Value::String("11 kB".into())],
            vec![Value::String("-11 kB".into())],
            vec![Value::Null]
        ]
    );
}

#[test]
fn should_resolve_stored_encodings_given_text_aliases_when_rows_are_filtered() {
    // Arrange
    let fixture = sql_fixture("catalog-encoding-stored", &[
        "CREATE TABLE scalar_encodings(value TEXT)",
        "INSERT INTO scalar_encodings VALUES ('UTF-8'),('not-an-encoding'),('windows-1252'),(NULL)",
    ]);
    // Act
    let result = fixture.execute("SELECT pg_char_to_encoding(value) AS output FROM scalar_encodings WHERE pg_char_to_encoding(value) >= 0 ORDER BY output").expect("stored encoding lookup");
    // Assert
    assert_eq!(result.columns()[0].type_oid, 23);
    assert_eq!(
        result.rows(),
        &[vec![Value::Int64(6)], vec![Value::Int64(24)]]
    );
}

#[test]
fn should_rebind_size_inputs_given_cached_plans_when_an_error_separates_valid_inputs() {
    // Arrange
    let fixture = sql_fixture("catalog-size-rebind", &[]);
    let sql = "SELECT pg_size_bytes($1::TEXT) AS output";
    // Act
    let first =
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![Value::String("1 kB".into())]);
    let invalid =
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![Value::String("bad".into())]);
    let null = fixture
        .cassie
        .execute_sql(&fixture.session, sql, vec![Value::Null]);
    let last =
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![Value::String("1.5 MB".into())]);
    // Assert
    assert_eq!(
        first.expect("first input").rows(),
        &[vec![Value::Int64(1024)]]
    );
    assert!(matches!(
        invalid,
        Err(CassieError::SizeInput(
            cassie::types::SizeInputError::InvalidSize(_)
        ))
    ));
    assert_eq!(null.expect("null input").rows(), &[vec![Value::Null]]);
    assert_eq!(
        last.expect("last input").rows(),
        &[vec![Value::Int64(1_572_864)]]
    );
}

fn physical(fixture: &SqlFixture) -> std::sync::Arc<cassie::planner::physical::PhysicalPlan> {
    let parsed =
        cassie::sql::parse_statement("SELECT pg_size_bytes($1::TEXT) AS output").expect("parse");
    let bound = cassie::sql::binder::bind(parsed, &fixture.cassie.catalog).expect("bind");
    let logical = cassie::planner::logical::plan(&bound).expect("plan");
    std::sync::Arc::new(cassie::planner::physical::build(logical))
}

#[test]
fn should_retain_size_results_given_a_result_clone_when_the_original_owner_drops() {
    // Arrange
    let fixture = sql_fixture("catalog-size-owner", &[]);
    let plan = physical(&fixture);
    let controls = QueryExecutionControls::from_limits(
        &cassie::config::CassieRuntimeConfig::default().limits,
        std::time::Instant::now(),
    );
    // Act
    let result = cassie::executor::run_with_controls(
        &fixture.cassie,
        &plan,
        vec![Value::String("1.5 MB".into())],
        &controls,
    )
    .expect("owned size result");
    let retained = result.clone();
    drop(result);
    // Assert
    assert_eq!(retained.rows(), &[vec![Value::Int64(1_572_864)]]);
    assert!(controls.current_query_memory_bytes() > 0);
    drop(retained);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_size_parsing_given_cancelled_controls_when_a_parameter_is_reached() {
    // Arrange
    let fixture = sql_fixture("catalog-size-cancel", &[]);
    let plan = physical(&fixture);
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();
    let controls = QueryExecutionControls::with_cancellation(
        &cassie::config::CassieRuntimeConfig::default().limits,
        std::time::Instant::now(),
        cancellation,
    );
    // Act
    let result = cassie::executor::run_with_controls(
        &fixture.cassie,
        &plan,
        vec![Value::String("1.5 MB".into())],
        &controls,
    );
    // Assert
    assert!(matches!(
        result.map_err(CassieError::from),
        Err(CassieError::QueryCancelled)
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
