//! CHECK bounds that are non-finite floats, overflow, or whose type has no
//! operator with the column are rejected at DDL time instead of being stored
//! as a bound that silently disables the constraint.

use super::support_sql_fixture::sql_fixture;

/// Runs `setup`, then `statement`, and returns `"ok"` or the SQLSTATE.
fn outcome(setup: &[&str], statement: &str) -> String {
    let fixture = sql_fixture("check-literal-types", setup);
    match fixture.execute(statement) {
        Ok(_) => "ok".to_string(),
        Err(error) => error.sql_state().to_string(),
    }
}

#[test]
fn should_reject_unquoted_nan_in_check() {
    // Arrange
    let setup = [];
    let statement = "CREATE TABLE tb (x DOUBLE PRECISION CHECK (x < NaN))";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "42703");
}

#[test]
fn should_reject_unquoted_infinity_in_check() {
    // Arrange
    let setup = [];
    let statement = "CREATE TABLE tb (x DOUBLE PRECISION CHECK (x < Infinity))";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "42703");
}

#[test]
fn should_reject_unquoted_inf_in_check() {
    // Arrange
    let setup = [];
    let statement = "CREATE TABLE tb (x DOUBLE PRECISION CHECK (x < inf))";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "42703");
}

#[test]
fn should_reject_unquoted_negative_infinity_in_check() {
    // Arrange
    let setup = [];
    let statement = "CREATE TABLE tb (x DOUBLE PRECISION CHECK (x > -Infinity))";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "42703");
}

#[test]
fn should_reject_unquoted_nan_in_added_constraint() {
    // Arrange
    let setup = ["CREATE TABLE tb (x DOUBLE PRECISION)"];
    let statement = "ALTER TABLE tb ADD CONSTRAINT c CHECK (x < NaN)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "42703");
}

#[test]
fn should_accept_finite_float_below_quoted_nan_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (x DOUBLE PRECISION CHECK (x < 'NaN'))"];
    let statement = "INSERT INTO tb (x) VALUES (5)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_finite_float_for_greater_than_quoted_nan_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (x DOUBLE PRECISION CHECK (x > 'NaN'))"];
    let statement = "INSERT INTO tb (x) VALUES (5)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "23514");
}

#[test]
fn should_accept_finite_float_for_not_equal_to_quoted_nan_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (x DOUBLE PRECISION CHECK (x <> 'NaN'))"];
    let statement = "INSERT INTO tb (x) VALUES (5)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_finite_float_equal_to_quoted_nan_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (x DOUBLE PRECISION CHECK (x = 'NaN'))"];
    let statement = "INSERT INTO tb (x) VALUES (5)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "23514");
}

#[test]
fn should_accept_finite_float_below_quoted_infinity_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (x DOUBLE PRECISION CHECK (x < 'Infinity'::float8))"];
    let statement = "INSERT INTO tb (x) VALUES (1e308)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_finite_float_for_greater_than_quoted_infinity_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (x DOUBLE PRECISION CHECK (x > 'Infinity'::float8))"];
    let statement = "INSERT INTO tb (x) VALUES (1e308)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "23514");
}

#[test]
fn should_accept_float_equal_to_text_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (x DOUBLE PRECISION CHECK (x = '1'))"];
    let statement = "INSERT INTO tb (x) VALUES (1.0)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_text_column_compared_with_integer() {
    // Arrange
    let setup = [];
    let statement = "CREATE TABLE tb (name TEXT CHECK (name = 5))";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "42883");
}

#[test]
fn should_reject_text_column_compared_with_numeric() {
    // Arrange
    let setup = [];
    let statement = "CREATE TABLE tb (name TEXT CHECK (name < 5.5))";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "42883");
}

#[test]
fn should_reject_integer_column_matched_with_like() {
    // Arrange
    let setup = [];
    let statement = "CREATE TABLE tb (n INT CHECK (n LIKE 'x%'))";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "42883");
}

#[test]
fn should_reject_date_column_compared_with_integer() {
    // Arrange
    let setup = [];
    let statement = "CREATE TABLE tb (d DATE CHECK (d = 5))";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "42883");
}

#[test]
fn should_reject_integer_column_compared_with_boolean() {
    // Arrange
    let setup = [];
    let statement = "CREATE TABLE tb (i INT CHECK (i = true))";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "42883");
}

#[test]
fn should_reject_text_column_compared_with_boolean() {
    // Arrange
    let setup = [];
    let statement = "CREATE TABLE tb (name TEXT CHECK (name = true))";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "42883");
}

#[test]
fn should_reject_added_constraint_comparing_text_with_integer() {
    // Arrange
    let setup = ["CREATE TABLE tb (name TEXT)"];
    let statement = "ALTER TABLE tb ADD CONSTRAINT c CHECK (name = 5)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "42883");
}

#[test]
fn should_reject_added_column_check_comparing_text_with_integer() {
    // Arrange
    let setup = ["CREATE TABLE tb (a INT)"];
    let statement = "ALTER TABLE tb ADD COLUMN name TEXT CHECK (name = 5)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "42883");
}

#[test]
fn should_accept_text_column_like_pattern() {
    // Arrange
    let setup = ["CREATE TABLE tb (name TEXT CHECK (name LIKE 'x%'))"];
    let statement = "INSERT INTO tb (name) VALUES ('xyz')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_accept_text_column_equal_to_text_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (name TEXT CHECK (name = 'abc'))"];
    let statement = "INSERT INTO tb (name) VALUES ('abc')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_accept_finite_float_above_quoted_negative_infinity_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (x DOUBLE PRECISION CHECK (x > '-Infinity'))"];
    let statement = "INSERT INTO tb (x) VALUES (-1e308)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_unquoted_nan_in_column_default() {
    // Arrange
    let setup: [&str; 0] = [];
    let statement = "CREATE TABLE tb (x DOUBLE PRECISION DEFAULT NaN)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_ne!(result, "ok");
}

#[test]
fn should_reject_decimal_default_that_overflows_a_float() {
    // Arrange
    let setup: [&str; 0] = [];
    let statement = "CREATE TABLE tb (x DOUBLE PRECISION DEFAULT 1e400)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_ne!(result, "ok");
}

#[test]
fn should_reject_decimal_check_bound_that_overflows_a_float() {
    // Arrange
    let setup: [&str; 0] = [];
    let statement = "CREATE TABLE tb (x DOUBLE PRECISION CHECK (x < 1e400))";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_ne!(result, "ok");
}
