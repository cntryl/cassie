//! Enum arrays: literals, columns and element comparison.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy')",
            "CREATE TABLE logs (id int, history mood[])",
            "INSERT INTO logs VALUES (1, '{sad,happy}'), (2, ARRAY['ok']::mood[]), (3, NULL)",
        ],
    )
}

#[test]
fn should_read_back_an_enum_array_column_as_labels() {
    // Arrange
    let fixture = fixture("enum_array_column");

    // Act
    let rows = fixture.rows("SELECT history::text FROM logs ORDER BY id");

    // Assert
    assert_eq!(
        rows,
        vec![
            vec![Value::String("{sad,happy}".to_string())],
            vec![Value::String("{ok}".to_string())],
            vec![Value::Null],
        ]
    );
}

#[test]
fn should_find_rows_whose_array_contains_a_label() {
    // Arrange
    let fixture = fixture("enum_array_any");

    // Act
    let rows = fixture.rows("SELECT id FROM logs WHERE 'happy' = ANY(history)");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_reject_an_unknown_label_in_an_array_literal() {
    // Arrange
    let fixture = fixture("enum_array_unknown");

    // Act
    let error = fixture.error("SELECT '{sad,furious}'::mood[]");

    // Assert
    assert_eq!(error.sql_state(), "22P02");
}

#[test]
fn should_find_a_label_position_in_an_enum_array() {
    // Arrange
    let fixture = fixture("enum_array_position");

    // Act
    let rows = fixture.rows(
        "SELECT array_position(history, 'happy'), array_positions(history, 'sad')::text \
         FROM logs WHERE id = 1",
    );
    let unknown = fixture.error("SELECT array_position(history, 'furious') FROM logs WHERE id = 1");

    // Assert
    assert_eq!(
        rows,
        vec![vec![Value::Int64(2), Value::String("{1}".to_string())]]
    );
    assert_eq!(unknown.sql_state(), "22P02");
}

#[test]
fn should_compare_an_enum_array_with_an_unknown_array_literal() {
    // Arrange
    let fixture = fixture("enum_array_literal_comparison");

    // Act
    let rows = fixture.rows("SELECT id FROM logs WHERE history = '{sad,happy}'");
    let unknown = fixture.error("SELECT id FROM logs WHERE history = '{sad,furious}'");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
    assert_eq!(unknown.sql_state(), "22P02");
}

#[test]
fn should_reject_a_text_array_constructor_compared_with_an_enum_with_42883() {
    // Arrange
    let fixture = sql_fixture(
        "enum_any_text_array",
        &[
            "CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy')",
            "CREATE TABLE days (id int, m mood)",
            "INSERT INTO days VALUES (1, 'ok')",
        ],
    );

    // Act
    let error = fixture.error("SELECT id FROM days WHERE m = ANY(ARRAY['zz'])");
    let typed = fixture.rows("SELECT id FROM days WHERE m = ANY(ARRAY['ok']::mood[])");

    // Assert
    assert_eq!(error.sql_state(), "42883");
    assert_eq!(typed, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_accept_a_multidimensional_enum_array_column_declaration() {
    // Arrange
    let fixture = sql_fixture(
        "enum_array_declared_dimensions",
        &[
            "CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy')",
            "CREATE TABLE grids (id int, cells mood[][])",
            "INSERT INTO grids VALUES (1, '{sad,happy}')",
        ],
    );

    // Act
    let rows = fixture.rows("SELECT cells::text FROM grids");

    // Assert
    assert_eq!(rows, vec![vec![Value::String("{sad,happy}".to_string())]]);
}
