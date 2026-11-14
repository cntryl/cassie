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
fn should_return_the_boundary_labels_of_the_type() {
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
fn should_treat_a_null_bound_as_open_ended() {
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
fn should_reject_boundaries_of_an_empty_enum_with_55000() {
    // Arrange
    let fixture = fixture("enum_first_empty");

    // Act
    let first = fixture.error("SELECT enum_first(NULL::nothing)");
    let last = fixture.error("SELECT enum_last(NULL::nothing)");

    // Assert
    assert_eq!(first.sql_state(), "55000");
    assert_eq!(last.sql_state(), "55000");
}

fn column_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy')",
            "CREATE TYPE other AS ENUM ('x', 'y')",
            "CREATE TABLE days (id int, m mood)",
            "INSERT INTO days VALUES (1, NULL), (2, 'ok')",
        ],
    )
}

#[test]
fn should_take_the_enum_type_from_a_column_whose_value_is_null() {
    // Arrange
    let fixture = column_fixture("enum_functions_null_column");

    // Act
    let rows = fixture.rows(
        "SELECT enum_first(m)::text, enum_last(m)::text, enum_range(m, NULL)::text \
         FROM days ORDER BY id",
    );

    // Assert
    let text = |value: &str| Value::String(value.to_string());
    assert_eq!(
        rows,
        vec![
            vec![text("sad"), text("happy"), text("{sad,ok,happy}")],
            vec![text("sad"), text("happy"), text("{ok,happy}")],
        ]
    );
}

#[test]
fn should_reject_enum_support_functions_over_two_enum_types_with_42883() {
    // Arrange
    let fixture = column_fixture("enum_functions_mixed_types");

    // Act
    let range = fixture.error("SELECT enum_range('sad'::mood, 'x'::other)");

    // Assert
    assert_eq!(range.sql_state(), "42883");
}

#[test]
fn should_pick_conditional_extremes_by_sort_order() {
    // Arrange
    let fixture = column_fixture("enum_conditional_extremes");

    // Act
    let greatest = single_text(
        &fixture,
        "SELECT greatest('happy'::mood, 'sad'::mood)::text",
    );
    let least = single_text(&fixture, "SELECT least('happy'::mood, 'ok'::mood)::text");
    let kept = single_text(&fixture, "SELECT nullif('ok'::mood, 'sad'::mood)::text");
    let nulled = fixture.rows("SELECT nullif('ok'::mood, 'ok'::mood)::text");

    // Assert
    assert_eq!(greatest, "happy");
    assert_eq!(least, "ok");
    assert_eq!(kept, "ok");
    assert_eq!(nulled, vec![vec![Value::Null]]);
}

#[test]
fn should_reject_conditional_extremes_over_two_enum_types_with_42846() {
    // Arrange
    let fixture = column_fixture("enum_conditional_mixed_types");

    // Act
    let error = fixture.error("SELECT greatest('sad'::mood, 'x'::other)");

    // Assert
    assert_eq!(error.sql_state(), "42846");
}
