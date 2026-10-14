//! PostgreSQL 18 mathematical operator grammar tracked by #813.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_evaluate_left_associative_postgres_power_with_float8_result() {
    // Arrange
    let fixture = sql_fixture("issue-813-left-associative-power", &[]);

    // Act
    let result = fixture.execute("SELECT 2 ^ 3 ^ 3 AS value");

    // Assert
    let result = result.expect("PostgreSQL power operator evaluates left-associatively");
    assert_eq!(result.rows(), &[vec![Value::Float64(512.0)]]);
    assert_eq!(result.columns()[0].type_oid, 701);
}

#[test]
fn should_evaluate_the_prefix_square_root_operator_as_float8() {
    // Arrange
    let fixture = sql_fixture("issue-813-prefix-square-root", &[]);

    // Act
    let result = fixture.execute("SELECT |/ 25.0 AS value");

    // Assert
    let result = result.expect("PostgreSQL prefix square-root operator");
    assert_eq!(result.rows(), &[vec![Value::Float64(5.0)]]);
    assert_eq!(result.columns()[0].type_oid, 701);
}

#[test]
fn should_evaluate_the_prefix_cube_root_operator_as_float8() {
    // Arrange
    let fixture = sql_fixture("issue-813-prefix-cube-root", &[]);

    // Act
    let result = fixture.execute("SELECT ||/ 64.0 AS value");

    // Assert
    let result = result.expect("PostgreSQL prefix cube-root operator");
    assert_eq!(result.rows(), &[vec![Value::Float64(4.0)]]);
    assert_eq!(result.columns()[0].type_oid, 701);
}
