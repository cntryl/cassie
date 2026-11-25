//! Enum casts and assignment: labels in, labels out, 42846 between families.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy')",
            "CREATE TYPE other AS ENUM ('sad')",
            "CREATE TABLE days (id int, m mood)",
        ],
    )
}

#[test]
fn should_round_trip_a_label_through_text_casts() {
    // Arrange
    let fixture = fixture("enum_cast_round_trip");

    // Act
    let rows = fixture.rows("SELECT CAST('ok' AS mood)::text, 'happy'::mood::varchar");

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::String("ok".to_string()),
            Value::String("happy".to_string())
        ]]
    );
}

#[test]
fn should_reject_an_unknown_label_cast_with_22p02() {
    // Arrange
    let fixture = fixture("enum_cast_unknown");

    // Act
    let error = fixture.error("SELECT 'furious'::mood");

    // Assert
    assert_eq!(error.sql_state(), "22P02");
}

#[test]
fn should_reject_non_text_casts_with_42846() {
    // Arrange
    let fixture = fixture("enum_cast_families");

    // Act
    let to_integer = fixture.error("SELECT 'ok'::mood::int");
    let from_integer = fixture.error("SELECT 1::mood");
    let cross = fixture.error("SELECT 'sad'::mood::other");

    // Assert
    assert_eq!(to_integer.sql_state(), "42846");
    assert_eq!(from_integer.sql_state(), "42846");
    assert_eq!(cross.sql_state(), "42846");
}

#[test]
fn should_assign_labels_on_writes() {
    // Arrange
    let fixture = fixture("enum_assign");
    fixture
        .execute("INSERT INTO days VALUES (1, 'sad')")
        .unwrap();

    // Act
    fixture
        .execute("UPDATE days SET m = 'happy' WHERE id = 1")
        .unwrap();
    let rows = fixture.rows("SELECT m::text FROM days");
    let error = fixture.error("INSERT INTO days VALUES (2, 'furious')");

    // Assert
    assert_eq!(rows, vec![vec![Value::String("happy".to_string())]]);
    assert_eq!(error.sql_state(), "22P02");
}

#[test]
fn should_reject_assigning_a_text_typed_expression_to_an_enum_column_with_42804() {
    // Arrange
    let fixture = sql_fixture(
        "enum_assignment_text_expression",
        &[
            "CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy')",
            "CREATE TYPE other AS ENUM ('sad')",
            "CREATE TABLE days (id int, m mood, t text)",
            "INSERT INTO days VALUES (1, 'ok', 'sad')",
        ],
    );

    // Act
    let update = fixture.error("UPDATE days SET m = t");
    let insert_select = fixture.error("INSERT INTO days (id, m) SELECT 2, t FROM days");
    let cast = fixture.error("INSERT INTO days (id, m) VALUES (3, 'sad'::text)");
    let other_enum = fixture.error("INSERT INTO days (id, m) VALUES (4, 'sad'::other)");
    let rows = fixture.rows("SELECT id, m::text FROM days ORDER BY id");

    // Assert
    assert_eq!(update.sql_state(), "42804");
    assert_eq!(insert_select.sql_state(), "42804");
    assert_eq!(cast.sql_state(), "42804");
    assert_eq!(other_enum.sql_state(), "42804");
    assert_eq!(
        rows,
        vec![vec![Value::Int64(1), Value::String("ok".to_string())]]
    );
}

#[test]
fn should_still_assign_same_enum_expressions_to_an_enum_column() {
    // Arrange
    let fixture = sql_fixture(
        "enum_assignment_allowed",
        &[
            "CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy')",
            "CREATE TABLE days (id int, m mood, previous mood)",
            "INSERT INTO days VALUES (1, 'ok', 'sad')",
        ],
    );

    // Act
    let from_column = fixture.execute("UPDATE days SET m = previous WHERE id = 1");
    let from_cast = fixture.execute("INSERT INTO days (id, m) VALUES (2, 'happy'::mood)");
    let from_select =
        fixture.execute("INSERT INTO days (id, m) SELECT 3, m FROM days WHERE id = 1");
    let rows = fixture.rows("SELECT m::text FROM days ORDER BY id");

    // Assert
    assert!(from_column.is_ok());
    assert!(from_cast.is_ok());
    assert!(from_select.is_ok());
    assert_eq!(
        rows,
        vec![
            vec![Value::String("sad".to_string())],
            vec![Value::String("happy".to_string())],
            vec![Value::String("sad".to_string())],
        ]
    );
}
