//! CHECK equality compares numbers by value, not by JSON representation.

use super::support_sql_fixture::sql_fixture;
use cassie::types::Value;

fn violates(setup: &[&str], insert: &str) -> bool {
    let fixture = sql_fixture("numeric-check-equality", setup);
    match fixture.execute(insert) {
        Ok(_) => false,
        Err(error) => {
            assert_eq!(error.sql_state(), "23514", "unexpected SQLSTATE");
            true
        }
    }
}

#[test]
fn should_accept_float_value_equal_to_integer_bound() {
    // Arrange
    let setup = ["CREATE TABLE cf (f DOUBLE PRECISION CHECK (f = 1))"];

    // Act
    let rejected = violates(&setup, "INSERT INTO cf (f) VALUES (1.0)");

    // Assert
    assert!(!rejected);
}

#[test]
fn should_reject_float_zero_for_not_equal_integer_bound() {
    // Arrange
    let setup = ["CREATE TABLE cn (f DOUBLE PRECISION CHECK (f <> 0))"];

    // Act
    let rejected = violates(&setup, "INSERT INTO cn (f) VALUES (0.0)");

    // Assert
    assert!(rejected);
}

#[test]
fn should_accept_integer_value_equal_to_float_bound() {
    // Arrange
    let setup = ["CREATE TABLE ci (i INT CHECK (i = 1.0))"];

    // Act
    let rejected = violates(&setup, "INSERT INTO ci (i) VALUES (1)");

    // Assert
    assert!(!rejected);
}

#[test]
fn should_accept_integer_value_not_equal_to_fractional_bound() {
    // Arrange
    let setup = ["CREATE TABLE cj (i INT CHECK (i <> 1.5))"];

    // Act
    let rejected = violates(&setup, "INSERT INTO cj (i) VALUES (1)");

    // Assert
    assert!(!rejected);
}

#[test]
fn should_reject_bigint_differing_from_bound_beyond_float_precision() {
    // Arrange
    let setup = ["CREATE TABLE cb (b BIGINT CHECK (b = 9007199254740993))"];

    // Act
    let rejected = violates(&setup, "INSERT INTO cb (b) VALUES (9007199254740992)");

    // Assert
    assert!(rejected);
}

#[test]
fn should_accept_numeric_value_equal_to_integer_bound() {
    // Arrange
    let setup = ["CREATE TABLE cm (n NUMERIC(10,2) CHECK (n = 1))"];

    // Act
    let rejected = violates(&setup, "INSERT INTO cm (n) VALUES (1.00)");

    // Assert
    assert!(!rejected);
}

#[test]
fn should_enforce_float_equality_check_on_update() {
    // Arrange
    let fixture = sql_fixture(
        "numeric-check-update",
        &[
            "CREATE TABLE cu (id INT, f DOUBLE PRECISION CHECK (f = 1))",
            "INSERT INTO cu (id, f) VALUES (1, 1.0)",
        ],
    );

    // Act
    let updated = fixture.execute("UPDATE cu SET f = 1.0 WHERE id = 1");

    // Assert
    updated.expect("1.0 equals bound 1");
    assert_eq!(
        fixture.rows("SELECT f FROM cu"),
        vec![vec![Value::Float64(1.0)]]
    );
    assert_eq!(
        fixture
            .error("UPDATE cu SET f = 2.0 WHERE id = 1")
            .sql_state(),
        "23514"
    );
}

#[test]
fn should_treat_float_equal_to_integer_bound_as_not_distinct() {
    // Arrange
    let setup = ["CREATE TABLE cd (f DOUBLE PRECISION CHECK (f IS NOT DISTINCT FROM 1))"];

    // Act
    let rejected = violates(&setup, "INSERT INTO cd (f) VALUES (1.0)");

    // Assert
    assert!(!rejected);
}

#[test]
fn should_reject_float_equal_to_integer_bound_for_is_distinct_from() {
    // Arrange
    let setup = ["CREATE TABLE ce (f DOUBLE PRECISION CHECK (f IS DISTINCT FROM 1))"];

    // Act
    let rejected = violates(&setup, "INSERT INTO ce (f) VALUES (1.0)");

    // Assert
    assert!(rejected);
}
