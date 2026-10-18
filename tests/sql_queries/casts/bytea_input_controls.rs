//! BYTEA input on stored rows, cached plans and owned query results.
use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::CassieError;
use cassie::runtime::{QueryCancellationHandle, QueryExecutionControls};
use cassie::types::Value;

#[test]
fn should_decode_stored_text_given_escape_input_when_a_row_is_cast_to_bytea() {
    // Arrange
    let fixture = sql_fixture(
        "byteainput-stored-text",
        &[
            "CREATE TABLE bi(v TEXT)",
            r"INSERT INTO bi SELECT 'a\000b\\c\377'::TEXT",
        ],
    );
    // Act
    let result = fixture.execute("SELECT v::BYTEA FROM bi");
    // Assert
    assert_eq!(
        result.expect("stored input").rows(),
        &[vec![Value::String(r"\x6100625c63ff".into())]]
    );
}

#[test]
fn should_preserve_stored_bytea_given_escape_assignment_when_the_column_is_read() {
    // Arrange
    let fixture = sql_fixture(
        "byteainput-stored-bytea",
        &[
            "CREATE TABLE bi(v BYTEA)",
            r"INSERT INTO bi SELECT '\101\000\377'::BYTEA",
        ],
    );
    // Act
    let result = fixture.execute("SELECT v FROM bi");
    // Assert
    assert_eq!(
        result.expect("stored BYTEA").rows(),
        &[vec![Value::String(r"\x4100ff".into())]]
    );
}

#[test]
fn should_reject_stored_text_given_an_invalid_escape_when_the_row_conversion_runs() {
    // Arrange
    let fixture = sql_fixture(
        "byteainput-stored-error",
        &[
            "CREATE TABLE bi(v TEXT)",
            r"INSERT INTO bi SELECT '\400'::TEXT",
        ],
    );
    // Act
    let result = fixture.execute("SELECT v::BYTEA FROM bi");
    // Assert
    assert!(matches!(
        result,
        Err(CassieError::ByteaInput(
            cassie::types::ByteaInputError::EscapeSyntax
        ))
    ));
}

#[test]
fn should_recover_cached_input_given_good_error_null_good_parameters_when_bytea_is_rebound() {
    // Arrange
    let fixture = sql_fixture("byteainput-cache", &[]);
    let sql = "SELECT $1::TEXT::BYTEA AS output";
    let parameters = [
        Value::String("abc".into()),
        Value::String(r"\400".into()),
        Value::Null,
        Value::String(r"\377".into()),
    ];
    // Act
    let mut results = parameters
        .map(|value| {
            fixture
                .cassie
                .execute_sql(&fixture.session, sql, vec![value])
        })
        .into_iter();
    // Assert
    assert_eq!(
        results.next().expect("first").expect("valid input").rows(),
        &[vec![Value::String(r"\x616263".into())]]
    );
    assert!(matches!(
        results.next().expect("second"),
        Err(CassieError::ByteaInput(_))
    ));
    assert_eq!(
        results.next().expect("third").expect("NULL").rows(),
        &[vec![Value::Null]]
    );
    assert_eq!(
        results.next().expect("fourth").expect("recovery").rows(),
        &[vec![Value::String(r"\xff".into())]]
    );
}

fn physical(fixture: &SqlFixture) -> std::sync::Arc<cassie::planner::physical::PhysicalPlan> {
    let parsed = cassie::sql::parse_statement("SELECT $1::TEXT::BYTEA AS output").expect("parse");
    let bound = cassie::sql::binder::bind(parsed, &fixture.cassie.catalog).expect("bind");
    let logical = cassie::planner::logical::plan(&bound).expect("plan");
    std::sync::Arc::new(cassie::planner::physical::build(logical))
}

#[test]
fn should_release_input_result_given_a_retained_clone_when_the_final_owner_drops() {
    // Arrange
    let fixture = sql_fixture("byteainput-owner", &[]);
    let plan = physical(&fixture);
    let controls = QueryExecutionControls::from_limits(
        &cassie::config::CassieRuntimeConfig::default().limits,
        std::time::Instant::now(),
    );
    // Act
    let result = cassie::executor::run_with_controls(
        &fixture.cassie,
        &plan,
        vec![Value::String("abc".into())],
        &controls,
    )
    .expect("owned result");
    let retained = result.clone();
    drop(result);
    // Assert
    assert_eq!(retained.rows(), &[vec![Value::String(r"\x616263".into())]]);
    assert!(controls.current_query_memory_bytes() > 0);
    drop(retained);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_input_conversion_given_cancelled_controls_when_an_escape_parameter_is_reached() {
    // Arrange
    let fixture = sql_fixture("byteainput-cancel", &[]);
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
        vec![Value::String(r"\377".into())],
        &controls,
    );
    // Assert
    assert!(matches!(
        result.map_err(CassieError::from),
        Err(CassieError::QueryCancelled)
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
