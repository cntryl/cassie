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
