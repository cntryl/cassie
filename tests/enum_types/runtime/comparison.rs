//! Enum comparison, ordering, grouping and min/max follow the sort order.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy')",
            "CREATE TYPE other AS ENUM ('sad')",
            "CREATE TABLE days (id int, m mood)",
            "INSERT INTO days VALUES (1, 'happy'), (2, 'sad'), (3, 'ok'), (4, 'sad'), (5, NULL)",
        ],
    )
}

fn texts(rows: &[Vec<Value>]) -> Vec<String> {
    rows.iter()
        .map(|row| match &row[0] {
            Value::String(text) => text.clone(),
            Value::Null => "NULL".to_string(),
            _ => panic!("expected a text or NULL cell"),
        })
        .collect()
}

#[test]
fn should_order_by_sort_order_not_label_text() {
    // Arrange
    let fixture = fixture("enum_order_by");

    // Act
    let rows = fixture.rows("SELECT m::text FROM days WHERE m IS NOT NULL ORDER BY m, id");

    // Assert
    assert_eq!(texts(&rows), vec!["sad", "sad", "ok", "happy"]);
}

#[test]
fn should_compare_with_an_unknown_literal_by_sort_order() {
    // Arrange
    let fixture = fixture("enum_range_compare");

    // Act
    let rows = fixture.rows("SELECT id FROM days WHERE m > 'sad' ORDER BY id");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)], vec![Value::Int64(3)]]);
}

#[test]
fn should_pick_extremes_by_sort_order() {
    // Arrange
    let fixture = fixture("enum_min_max");

    // Act
    let rows = fixture.rows("SELECT min(m)::text, max(m)::text FROM days");

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::String("sad".to_string()),
            Value::String("happy".to_string())
        ]]
    );
}

#[test]
fn should_collapse_equal_members_when_grouping() {
    // Arrange
    let fixture = fixture("enum_group_distinct");

    // Act
    let groups = fixture.rows("SELECT m::text, count(*) FROM days GROUP BY m ORDER BY m");
    let distinct = fixture.rows("SELECT DISTINCT m::text FROM days WHERE m IS NOT NULL ORDER BY m");

    // Assert
    assert_eq!(groups.len(), 4);
    assert_eq!(texts(&distinct), vec!["sad", "ok", "happy"]);
}

#[test]
fn should_match_lists_of_labels() {
    // Arrange
    let fixture = fixture("enum_in_any");

    // Act
    let in_list = fixture.rows("SELECT id FROM days WHERE m IN ('ok', 'happy') ORDER BY id");
    let any = fixture.rows("SELECT id FROM days WHERE m = ANY('{sad,ok}') ORDER BY id");

    // Assert
    assert_eq!(in_list.len(), 2);
    assert_eq!(any.len(), 3);
}

#[test]
fn should_reject_foreign_operand_types_with_42883() {
    // Arrange
    let fixture = fixture("enum_operator_errors");

    // Act
    let text = fixture.error("SELECT 1 FROM days WHERE m = 'ok'::text");
    let different = fixture.error("SELECT 1 FROM days WHERE m = 'sad'::other");

    // Assert
    assert_eq!(text.sql_state(), "42883");
    assert_eq!(different.sql_state(), "42883");
}

#[test]
fn should_reject_an_unknown_literal_label_with_22p02() {
    // Arrange
    let fixture = fixture("enum_unknown_literal");

    // Act
    let error = fixture.error("SELECT 1 FROM days WHERE m = 'furious'");

    // Assert
    assert_eq!(error.sql_state(), "22P02");
}
