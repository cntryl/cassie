//! Explicit casts preserve strict assignment and cache boundaries.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_reject_ascii_given_a_varchar_column_when_assignment_exceeds_its_width() {
    // Arrange
    let fixture = sql_fixture(
        "varchar-assignment-ascii",
        &["CREATE TABLE vc(v VARCHAR(3))"],
    );
    // Act
    let result = fixture.execute("INSERT INTO vc VALUES ('abcdef')");
    // Assert
    let error = result.expect_err("strict assignment");
    assert!(matches!(
        error,
        cassie::app::CassieError::Diagnostic(ref diagnostic)
            if diagnostic.sql_state == "22001"
                && diagnostic.message == "value too long for type character varying(3)"
    ));
    assert_eq!(fixture.rows("SELECT v FROM vc"), Vec::<Vec<Value>>::new());
}

#[test]
fn should_store_ascii_given_an_explicit_varchar_cast_when_assignment_receives_truncated_text() {
    // Arrange
    let fixture = sql_fixture(
        "varchar-cast-assignment-ascii",
        &["CREATE TABLE vc(v VARCHAR(3))"],
    );
    // Act
    let result = fixture.execute("INSERT INTO vc SELECT 'abcdef'::VARCHAR(3)");
    // Assert
    result.expect("explicit cast insertion");
    assert_eq!(
        fixture.rows("SELECT v FROM vc"),
        vec![vec![Value::String("abc".into())]]
    );
}

#[test]
fn should_reject_unicode_given_a_varchar_column_when_assignment_exceeds_its_width() {
    // Arrange
    let fixture = sql_fixture(
        "varchar-assignment-unicode",
        &["CREATE TABLE vc(v VARCHAR(3))"],
    );
    // Act
    let result = fixture.execute("INSERT INTO vc VALUES ('a🙂文z')");
    // Assert
    let error = result.expect_err("strict assignment");
    assert!(matches!(
        error,
        cassie::app::CassieError::Diagnostic(ref diagnostic)
            if diagnostic.sql_state == "22001"
                && diagnostic.message == "value too long for type character varying(3)"
    ));
    assert_eq!(fixture.rows("SELECT v FROM vc"), Vec::<Vec<Value>>::new());
}

#[test]
fn should_store_unicode_given_an_explicit_varchar_cast_when_assignment_receives_truncated_text() {
    // Arrange
    let fixture = sql_fixture(
        "varchar-cast-assignment-unicode",
        &["CREATE TABLE vc(v VARCHAR(3))"],
    );
    // Act
    let result = fixture.execute("INSERT INTO vc SELECT 'a🙂文z'::VARCHAR(3)");
    // Assert
    result.expect("explicit cast insertion");
    assert_eq!(
        fixture.rows("SELECT v FROM vc"),
        vec![vec![Value::String("a🙂文".into())]]
    );
}

#[test]
fn should_preserve_null_given_a_varchar_column_when_cast_assignment_is_executed() {
    // Arrange
    let fixture = sql_fixture(
        "varchar-null-assignment",
        &["CREATE TABLE vc(v VARCHAR(3))"],
    );
    // Act
    let result = fixture.execute("INSERT INTO vc SELECT NULL::VARCHAR(3)");
    // Assert
    result.expect("NULL cast insertion");
    assert_eq!(fixture.rows("SELECT v FROM vc"), vec![vec![Value::Null]]);
}

#[test]
fn should_rebind_text_given_a_cached_varchar_cast_when_parameter_values_change() {
    // Arrange
    let fixture = sql_fixture("varchar-cache", &[]);
    let sql = "SELECT $1::VARCHAR(3) AS output";
    // Act
    let first =
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![Value::String("abcdef".into())]);
    let second =
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![Value::String("🙂文xy".into())]);
    let third = fixture
        .cassie
        .execute_sql(&fixture.session, sql, vec![Value::Null]);
    // Assert
    assert_eq!(
        first.expect("first cast").rows(),
        &[vec![Value::String("abc".into())]]
    );
    assert_eq!(
        second.expect("second cast").rows(),
        &[vec![Value::String("🙂文x".into())]]
    );
    assert_eq!(third.expect("NULL cast").rows(), &[vec![Value::Null]]);
}

#[test]
fn should_bound_output_given_a_large_varchar_typmod_when_input_is_short() {
    // Arrange
    let fixture = sql_fixture("varchar-large-width", &[]);
    let parsed =
        cassie::sql::parse_statement("SELECT $1::VARCHAR(100000) AS output").expect("parse");
    let bound = cassie::sql::binder::bind(parsed, &fixture.cassie.catalog).expect("bind");
    let logical = cassie::planner::logical::plan(&bound).expect("plan");
    let plan = std::sync::Arc::new(cassie::planner::physical::build(logical));
    let mut limits = cassie::config::CassieRuntimeConfig::default().limits;
    limits.query_memory_budget_bytes = 16384;
    let controls =
        cassie::runtime::QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
    // Act
    let result = cassie::executor::run_with_controls(
        &fixture.cassie,
        &plan,
        vec![Value::String("a".into())],
        &controls,
    )
    .expect("short input fits small budget");
    // Assert
    assert_eq!(result.rows(), &[vec![Value::String("a".into())]]);
    drop(result);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_truncate_after_inner_cast_given_nested_varchar_typmods_when_the_outer_width_is_smaller() {
    // Arrange
    let fixture = sql_fixture("varchar-nested", &[]);
    // Act
    let result = fixture.execute("SELECT 'a🙂文xyz'::VARCHAR(5)::VARCHAR(2)::TEXT");
    // Assert
    assert_eq!(
        result.expect("nested cast").rows(),
        &[vec![Value::String("a🙂".into())]]
    );
}
