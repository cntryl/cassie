//! Unary operators must use the same checked semantics for stored rows.
use crate::support_sql_fixture::sql_fixture;
use cassie::app::CassieError;
use cassie::types::Value;

#[test]
fn should_negate_stored_int4_given_projection_when_rows_are_read() {
    // Arrange
    let fixture = sql_fixture(
        "unary-int4-rows",
        &[
            "CREATE TABLE unary_rows (v INT)",
            "INSERT INTO unary_rows VALUES (1),(7)",
        ],
    );
    // Act
    let result = fixture
        .execute("SELECT -(v) FROM unary_rows ORDER BY v")
        .expect("projection");
    // Assert
    assert_eq!(
        result.rows(),
        &[vec![Value::Int64(-1)], vec![Value::Int64(-7)]]
    );
}
#[test]
fn should_reject_stored_int2_overflow_given_projection_when_the_minimum_is_negated() {
    // Arrange
    let fixture = sql_fixture(
        "unary-int2-min",
        &[
            "CREATE TABLE unary_rows (v SMALLINT)",
            "INSERT INTO unary_rows VALUES (-32768)",
        ],
    );
    // Act
    let result = fixture.execute("SELECT -(v) FROM unary_rows");
    // Assert
    assert!(
        matches!(result, Err(CassieError::Execution(message)) if message == "smallint out of range")
    );
}
#[test]
fn should_reject_stored_int4_overflow_given_projection_when_the_minimum_is_negated() {
    // Arrange
    let fixture = sql_fixture(
        "unary-int4-min",
        &[
            "CREATE TABLE unary_rows (v INT)",
            "INSERT INTO unary_rows VALUES (-2147483648)",
        ],
    );
    // Act
    let result = fixture.execute("SELECT -(v) FROM unary_rows");
    // Assert
    assert!(
        matches!(result, Err(CassieError::Execution(message)) if message == "integer out of range")
    );
}
#[test]
fn should_reject_stored_int8_overflow_given_projection_when_the_minimum_is_negated() {
    // Arrange
    let fixture = sql_fixture(
        "unary-int8-min",
        &[
            "CREATE TABLE unary_rows (v BIGINT)",
            "INSERT INTO unary_rows VALUES (-9223372036854775808)",
        ],
    );
    // Act
    let result = fixture.execute("SELECT -(v) FROM unary_rows");
    // Assert
    assert!(
        matches!(result, Err(CassieError::Execution(message)) if message == "bigint out of range")
    );
}
#[test]
fn should_filter_stored_rows_given_unary_predicates_when_numeric_signs_are_applied() {
    // Arrange
    let fixture = sql_fixture(
        "unary-filter",
        &[
            "CREATE TABLE unary_rows (v INT)",
            "INSERT INTO unary_rows VALUES (-1),(1),(7)",
        ],
    );
    // Act
    let result = fixture
        .execute("SELECT v FROM unary_rows WHERE -(v)<0 ORDER BY v")
        .expect("filter");
    // Assert
    assert_eq!(
        result.rows(),
        &[vec![Value::Int64(1)], vec![Value::Int64(7)]]
    );
}
#[test]
fn should_aggregate_stored_rows_given_unary_arguments_when_sums_are_computed() {
    // Arrange
    let fixture = sql_fixture(
        "unary-sum",
        &[
            "CREATE TABLE unary_rows (v INT)",
            "INSERT INTO unary_rows VALUES (1),(7)",
        ],
    );
    // Act
    let result = fixture
        .execute("SELECT SUM(-(v)) FROM unary_rows")
        .expect("sum");
    // Assert
    assert_eq!(result.rows(), &[vec![Value::Int64(-8)]]);
}
#[test]
fn should_preserve_stored_nulls_given_unary_projection_when_integer_columns_are_nullable() {
    // Arrange
    let fixture = sql_fixture(
        "unary-null",
        &[
            "CREATE TABLE unary_rows (v INT)",
            "INSERT INTO unary_rows VALUES (NULL)",
        ],
    );
    // Act
    let result = fixture
        .execute("SELECT -(v), +(v) FROM unary_rows")
        .expect("null signs");
    // Assert
    assert_eq!(result.rows(), &[vec![Value::Null, Value::Null]]);
}
#[test]
fn should_preserve_cached_unary_results_given_repeated_queries_when_signs_differ() {
    // Arrange
    let fixture = sql_fixture(
        "unary-cache",
        &[
            "CREATE TABLE unary_rows (v INT)",
            "INSERT INTO unary_rows VALUES (7)",
        ],
    );
    // Act
    let minus = fixture
        .execute("SELECT -(v) FROM unary_rows")
        .expect("minus");
    let plus = fixture
        .execute("SELECT +(v) FROM unary_rows")
        .expect("plus");
    let repeated = fixture
        .execute("SELECT -(v) FROM unary_rows")
        .expect("cached minus");
    // Assert
    assert_eq!(minus.rows(), &[vec![Value::Int64(-7)]]);
    assert_eq!(plus.rows(), &[vec![Value::Int64(7)]]);
    assert_eq!(repeated.rows(), minus.rows());
}
