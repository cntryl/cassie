//! BETWEEN and IN bind tighter than the comparison operators, as in PostgreSQL.
use super::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_bind_between_tighter_than_equality() {
    // Arrange
    let fixture = sql_fixture("between-precedence", &[]);

    // Act
    let rows = fixture.rows("SELECT false = true BETWEEN false AND true");

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(false)]]);
}

#[test]
fn should_bind_not_between_tighter_than_equality() {
    // Arrange
    let fixture = sql_fixture("not-between-precedence", &[]);

    // Act
    let rows = fixture.rows("SELECT false = false NOT BETWEEN false AND true");

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_bind_in_tighter_than_equality() {
    // Arrange
    let fixture = sql_fixture("in-precedence", &[]);

    // Act
    let rows = fixture.rows("SELECT false = true IN (false, true)");

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(false)]]);
}

#[test]
fn should_bind_not_in_tighter_than_inequality() {
    // Arrange
    let fixture = sql_fixture("not-in-precedence", &[]);

    // Act
    let rows = fixture.rows("SELECT true <> true NOT IN (true)");

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_keep_between_over_an_arithmetic_left_operand() {
    // Arrange
    let fixture = sql_fixture("between-arithmetic-operand", &[]);

    // Act
    let rows = fixture.rows("SELECT 1 + 2 BETWEEN 3 AND 3");

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_filter_rows_by_a_comparison_against_a_between_result() {
    // Arrange
    let fixture = sql_fixture(
        "between-precedence-filter",
        &[
            "CREATE TABLE flags (n BIGINT, ok BOOLEAN)",
            "INSERT INTO flags VALUES (1, true), (5, true), (2, false), (9, false)",
        ],
    );

    // Act
    let rows = fixture.rows("SELECT n FROM flags WHERE ok = n BETWEEN 1 AND 3 ORDER BY n");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)], vec![Value::Int64(9)]]);
}
