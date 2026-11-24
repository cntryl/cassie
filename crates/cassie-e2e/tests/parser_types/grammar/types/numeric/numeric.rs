//! PostgreSQL 18 NUMERIC and DECIMAL cast grammar tracked by #812.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_round_numeric_values_at_declared_scale() {
    // Arrange
    let fixture = sql_fixture("issue-812-numeric-parser-scale", &[]);

    // Act
    let result = fixture.execute("SELECT CAST('12.345' AS NUMERIC(5, 2))");

    // Assert
    let result = result.expect("NUMERIC precision and scale cast");
    // Native NUMERIC results publish PostgreSQL's text output.
    assert_eq!(result.rows(), &[vec![Value::String("12.35".to_owned())]]);
    assert_eq!(result.columns()[0].type_oid, 1700);
}

#[test]
fn should_round_decimal_values_at_negative_scale() {
    // Arrange
    let fixture = sql_fixture("issue-812-decimal-parser-negative-scale", &[]);

    // Act
    let result = fixture.execute("SELECT CAST('12500' AS DECIMAL(2, -3))");

    // Assert
    let result = result.expect("DECIMAL negative-scale cast");
    // Native NUMERIC results publish PostgreSQL's text output.
    assert_eq!(result.rows(), &[vec![Value::String("13000".to_owned())]]);
    assert_eq!(result.columns()[0].type_oid, 1700);
}
