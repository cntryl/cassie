//! An unknown literal meeting an enum in CASE, COALESCE, GREATEST, LEAST and
//! NULLIF is read as a label of that enum, as in PostgreSQL 18.6.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy')",
            "CREATE TABLE days (id int, m mood)",
            "INSERT INTO days VALUES (1, 'happy'), (2, 'sad'), (3, NULL)",
        ],
    )
}

fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}

#[test]
fn should_compare_a_simple_case_operand_with_label_literals() {
    // Arrange
    let fixture = fixture("enum_simple_case_labels");

    // Act
    let rows = fixture.rows(
        "SELECT CASE m WHEN 'sad' THEN 'low' WHEN 'happy' THEN 'high' ELSE 'none' END \
         FROM days ORDER BY id",
    );

    // Assert
    assert_eq!(
        rows,
        vec![vec![text("high")], vec![text("low")], vec![text("none")]]
    );
}

#[test]
fn should_resolve_a_coalesce_label_literal_to_the_enum() {
    // Arrange
    let fixture = fixture("enum_coalesce_label");

    // Act
    let rows = fixture.rows(
        "SELECT coalesce(m, 'ok')::text, pg_typeof(coalesce(m, 'ok'))::text FROM days ORDER BY id",
    );

    // Assert
    assert_eq!(
        rows,
        vec![
            vec![text("happy"), text("mood")],
            vec![text("sad"), text("mood")],
            vec![text("ok"), text("mood")],
        ]
    );
}

#[test]
fn should_order_greatest_and_least_label_literals_by_sort_order() {
    // Arrange
    let fixture = fixture("enum_greatest_least_labels");

    // Act
    let rows =
        fixture.rows("SELECT greatest(m, 'ok')::text, least(m, 'ok')::text FROM days ORDER BY id");

    // Assert
    assert_eq!(
        rows,
        vec![
            vec![text("happy"), text("ok")],
            vec![text("ok"), text("sad")],
            vec![text("ok"), text("ok")],
        ]
    );
}

#[test]
fn should_resolve_case_result_literals_to_the_enum_of_another_branch() {
    // Arrange
    let fixture = fixture("enum_case_result_label");

    // Act
    let rows = fixture.rows(
        "SELECT (CASE WHEN id = 1 THEN m ELSE 'ok' END)::text, nullif(m, 'sad')::text \
         FROM days ORDER BY id",
    );

    // Assert
    assert_eq!(
        rows,
        vec![
            vec![text("happy"), text("happy")],
            vec![text("ok"), Value::Null],
            vec![text("ok"), Value::Null],
        ]
    );
}

#[test]
fn should_reject_a_conditional_literal_that_is_not_a_label_with_sqlstate_22p02() {
    // Arrange
    let fixture = fixture("enum_conditional_bad_label");

    // Act
    let coalesce = fixture.error("SELECT coalesce(m, 'nope') FROM days");
    let case = fixture.error("SELECT CASE m WHEN 'nope' THEN 1 END FROM days");

    // Assert
    assert_eq!(coalesce.sql_state(), "22P02");
    assert_eq!(case.sql_state(), "22P02");
}
