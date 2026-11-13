//! `enum_first`, `enum_last` and `enum_range`.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy')",
            "CREATE TYPE nothing AS ENUM ()",
        ],
    )
}

fn single_text(fixture: &SqlFixture, sql: &str) -> String {
    match &fixture.rows(sql)[0][0] {
        Value::String(text) => text.clone(),
        _ => panic!("expected a text cell"),
    }
}

#[test]
fn should_return_the_first_and_last_labels_of_the_type() {
    // Arrange
    let fixture = fixture("enum_first_last");

    // Act
    let first = single_text(&fixture, "SELECT enum_first(NULL::mood)::text");
    let last = single_text(&fixture, "SELECT enum_last(NULL::mood)::text");

    // Assert
    assert_eq!(first, "sad");
    assert_eq!(last, "happy");
}

#[test]
fn should_return_the_whole_range_in_sort_order() {
    // Arrange
    let fixture = fixture("enum_range_all");

    // Act
    let range = single_text(&fixture, "SELECT enum_range(NULL::mood)::text");

    // Assert
    assert_eq!(range, "{sad,ok,happy}");
}

#[test]
fn should_treat_a_null_bound_as_open_ended_and_empty_when_reversed() {
    // Arrange
    let fixture = fixture("enum_range_bounds");

    // Act
    let from_ok = single_text(&fixture, "SELECT enum_range('ok'::mood, NULL)::text");
    let to_ok = single_text(&fixture, "SELECT enum_range(NULL, 'ok'::mood)::text");
    let reversed = single_text(
        &fixture,
        "SELECT enum_range('happy'::mood, 'sad'::mood)::text",
    );

    // Assert
    assert_eq!(from_ok, "{ok,happy}");
    assert_eq!(to_ok, "{sad,ok}");
    assert_eq!(reversed, "{}");
}

#[test]
fn should_reject_first_and_last_of_an_empty_enum_with_55000() {
    // Arrange
    let fixture = fixture("enum_first_empty");

    // Act
    let first = fixture.error("SELECT enum_first(NULL::nothing)");
    let last = fixture.error("SELECT enum_last(NULL::nothing)");

    // Assert
    assert_eq!(first.sql_state(), "55000");
    assert_eq!(last.sql_state(), "55000");
}
