//! Typed integer/BYTEA NULLs, stored columns, cache and query ownership.
use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::CassieError;
use cassie::runtime::{QueryCancellationHandle, QueryExecutionControls};
use cassie::types::Value;

#[test]
fn should_preserve_int2_encode_given_null_input_when_the_cast_is_evaluated() {
    // Arrange
    let fixture = sql_fixture("byteacasts-null-int2-encode", &[]);
    // Act
    let result = fixture.execute("SELECT NULL::INT2::BYTEA");
    // Assert
    assert_eq!(result.expect("NULL cast").rows(), &[vec![Value::Null]]);
}

#[test]
fn should_preserve_int2_decode_given_null_input_when_the_cast_is_evaluated() {
    // Arrange
    let fixture = sql_fixture("byteacasts-null-int2-decode", &[]);
    // Act
    let result = fixture.execute("SELECT NULL::BYTEA::INT2");
    // Assert
    assert_eq!(result.expect("NULL cast").rows(), &[vec![Value::Null]]);
}

#[test]
fn should_preserve_int2_width_given_a_stored_column_when_bytea_is_projected() {
    // Arrange
    let fixture = sql_fixture(
        "byteacasts-column-int2",
        &["CREATE TABLE bc(v INT2)", "INSERT INTO bc VALUES (-1)"],
    );
    // Act
    let result = fixture.execute("SELECT v::BYTEA FROM bc");
    // Assert
    let result = result.expect("stored integer cast");
    assert_eq!(result.columns()[0].type_oid, 17);
    assert_eq!(result.rows(), &[vec![Value::String("\\xffff".into())]]);
}

#[test]
fn should_preserve_int4_encode_given_null_input_when_the_cast_is_evaluated() {
    // Arrange
    let fixture = sql_fixture("byteacasts-null-int4-encode", &[]);
    // Act
    let result = fixture.execute("SELECT NULL::INT4::BYTEA");
    // Assert
    assert_eq!(result.expect("NULL cast").rows(), &[vec![Value::Null]]);
}

#[test]
fn should_preserve_int4_decode_given_null_input_when_the_cast_is_evaluated() {
    // Arrange
    let fixture = sql_fixture("byteacasts-null-int4-decode", &[]);
    // Act
    let result = fixture.execute("SELECT NULL::BYTEA::INT4");
    // Assert
    assert_eq!(result.expect("NULL cast").rows(), &[vec![Value::Null]]);
}

#[test]
fn should_preserve_int4_width_given_a_stored_column_when_bytea_is_projected() {
    // Arrange
    let fixture = sql_fixture(
        "byteacasts-column-int4",
        &["CREATE TABLE bc(v INT4)", "INSERT INTO bc VALUES (-1)"],
    );
    // Act
    let result = fixture.execute("SELECT v::BYTEA FROM bc");
    // Assert
    let result = result.expect("stored integer cast");
    assert_eq!(result.columns()[0].type_oid, 17);
    assert_eq!(result.rows(), &[vec![Value::String("\\xffffffff".into())]]);
}

#[test]
fn should_preserve_int8_encode_given_null_input_when_the_cast_is_evaluated() {
    // Arrange
    let fixture = sql_fixture("byteacasts-null-int8-encode", &[]);
    // Act
    let result = fixture.execute("SELECT NULL::INT8::BYTEA");
    // Assert
    assert_eq!(result.expect("NULL cast").rows(), &[vec![Value::Null]]);
}

#[test]
fn should_preserve_int8_decode_given_null_input_when_the_cast_is_evaluated() {
    // Arrange
    let fixture = sql_fixture("byteacasts-null-int8-decode", &[]);
    // Act
    let result = fixture.execute("SELECT NULL::BYTEA::INT8");
    // Assert
    assert_eq!(result.expect("NULL cast").rows(), &[vec![Value::Null]]);
}

#[test]
fn should_preserve_int8_width_given_a_stored_column_when_bytea_is_projected() {
    // Arrange
    let fixture = sql_fixture(
        "byteacasts-column-int8",
        &["CREATE TABLE bc(v INT8)", "INSERT INTO bc VALUES (-1)"],
    );
    // Act
    let result = fixture.execute("SELECT v::BYTEA FROM bc");
    // Assert
    let result = result.expect("stored integer cast");
    assert_eq!(result.columns()[0].type_oid, 17);
    assert_eq!(
        result.rows(),
        &[vec![Value::String("\\xffffffffffffffff".into())]]
    );
}

fn physical(
    fixture: &SqlFixture,
    sql: &str,
) -> std::sync::Arc<cassie::planner::physical::PhysicalPlan> {
    let parsed = cassie::sql::parse_statement(sql).expect("parse");
    let bound = cassie::sql::binder::bind(parsed, &fixture.cassie.catalog).expect("bind");
    let logical = cassie::planner::logical::plan(&bound).expect("plan");
    std::sync::Arc::new(cassie::planner::physical::build(logical))
}

#[test]
fn should_rebind_bytea_given_a_cached_integer_cast_when_signed_values_change() {
    // Arrange
    let fixture = sql_fixture("byteacasts-cache", &[]);
    let sql = "SELECT $1::INT2::BYTEA AS output";
    // Act
    let results = [-1, 0, 32767].map(|value| {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![Value::Int64(value)])
    });
    let null = fixture
        .cassie
        .execute_sql(&fixture.session, sql, vec![Value::Null]);
    // Assert
    for (result, expected) in results.into_iter().zip([r"\xffff", r"\x0000", r"\x7fff"]) {
        assert_eq!(
            result.expect("cached cast").rows(),
            &[vec![Value::String(expected.into())]]
        );
    }
    assert_eq!(null.expect("NULL cache input").rows(), &[vec![Value::Null]]);
}

#[test]
fn should_retain_bytea_given_result_clones_when_the_original_owner_drops() {
    // Arrange
    let fixture = sql_fixture("byteacasts-owner", &[]);
    let plan = physical(&fixture, "SELECT $1::INT8::BYTEA AS output");
    let limits = cassie::config::CassieRuntimeConfig::default().limits;
    let controls = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
    // Act
    let result = cassie::executor::run_with_controls(
        &fixture.cassie,
        &plan,
        vec![Value::Int64(-1)],
        &controls,
    )
    .expect("cast");
    let retained = result.clone();
    drop(result);
    // Assert
    assert_eq!(retained.columns()[0].type_oid, 17);
    assert_eq!(
        retained.rows(),
        &[vec![Value::String(r"\xffffffffffffffff".into())]]
    );
    assert!(controls.current_query_memory_bytes() > 0);
    drop(retained);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_bytea_given_cancelled_controls_when_integer_conversion_starts() {
    // Arrange
    let fixture = sql_fixture("byteacasts-cancel", &[]);
    let plan = physical(&fixture, "SELECT $1::INT8::BYTEA AS output");
    let limits = cassie::config::CassieRuntimeConfig::default().limits;
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();
    let controls =
        QueryExecutionControls::with_cancellation(&limits, std::time::Instant::now(), cancellation);
    // Act
    let result = cassie::executor::run_with_controls(
        &fixture.cassie,
        &plan,
        vec![Value::Int64(-1)],
        &controls,
    );
    // Assert
    assert!(matches!(
        result.map_err(CassieError::from),
        Err(CassieError::QueryCancelled)
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_bytea_given_changed_budgets_when_the_same_integer_plan_is_retried() {
    // Arrange
    let fixture = sql_fixture("byteacasts-admission", &[]);
    let plan = physical(&fixture, "SELECT $1::INT8::BYTEA AS output");
    let mut limits = cassie::config::CassieRuntimeConfig::default().limits;
    limits.query_memory_budget_bytes = 1;
    let denied = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
    limits.query_memory_budget_bytes = 16384;
    let admitted = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
    // Act
    let rejected =
        cassie::executor::run_with_controls(&fixture.cassie, &plan, vec![Value::Int64(1)], &denied);
    let result = cassie::executor::run_with_controls(
        &fixture.cassie,
        &plan,
        vec![Value::Int64(1)],
        &admitted,
    );
    // Assert
    assert!(matches!(
        rejected.map_err(CassieError::from),
        Err(CassieError::ResourceLimit(_))
    ));
    assert_eq!(denied.current_query_memory_bytes(), 0);
    let result = result.expect("bounded integer output fits 16KiB");
    assert_eq!(
        result.rows(),
        &[vec![Value::String(r"\x0000000000000001".into())]]
    );
    drop(result);
    assert_eq!(admitted.current_query_memory_bytes(), 0);
}
