//! `setval` coerces unknown-typed string literals like PostgreSQL.

use cassie::types::Value;

use super::support_sql_fixture::{sql_fixture, SqlFixture};

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(label, &["CREATE SEQUENCE sq"])
}

#[test]
fn should_set_a_sequence_to_a_quoted_integer() {
    // Arrange
    let fixture = fixture("setval-quoted-value");

    // Act
    let rows = fixture.rows("SELECT setval('sq', '5')");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(5)]]);
    assert_eq!(
        fixture.rows("SELECT nextval('sq')"),
        vec![vec![Value::Int64(6)]]
    );
}

#[test]
fn should_set_a_sequence_to_a_quoted_integer_with_surrounding_spaces() {
    // Arrange
    let fixture = fixture("setval-quoted-spaces");

    // Act
    let rows = fixture.rows("SELECT setval('sq', ' 7 ')");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(7)]]);
}

#[test]
fn should_reject_a_quoted_value_that_is_not_an_integer() {
    // Arrange
    let fixture = fixture("setval-bad-text");

    // Act
    let error = fixture.error("SELECT setval('sq', 'x')");

    // Assert
    assert_eq!(error.sql_state(), "22P02");
}

#[test]
fn should_reject_a_quoted_fraction() {
    // Arrange
    let fixture = fixture("setval-fraction-text");

    // Act
    let error = fixture.error("SELECT setval('sq', '5.5')");

    // Assert
    assert_eq!(error.sql_state(), "22P02");
}

#[test]
fn should_accept_a_quoted_boolean_for_is_called() {
    // Arrange
    let fixture = fixture("setval-quoted-flag");

    // Act
    let rows = fixture.rows("SELECT setval('sq', 5, 'false')");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(5)]]);
    assert_eq!(
        fixture.rows("SELECT nextval('sq')"),
        vec![vec![Value::Int64(5)]]
    );
}

#[test]
fn should_reject_a_quoted_is_called_that_is_not_a_boolean() {
    // Arrange
    let fixture = fixture("setval-bad-flag");

    // Act
    let error = fixture.error("SELECT setval('sq', 5, 'maybe')");

    // Assert
    assert_eq!(error.sql_state(), "22P02");
}
