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
    let rows = fixture.rows("SELECT m::text AS label FROM days WHERE m IS NOT NULL ORDER BY m, id");

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
    let groups = fixture.rows("SELECT m::text AS label, count(*) FROM days GROUP BY m ORDER BY m");
    let distinct = fixture.rows("SELECT m::text AS label FROM (SELECT DISTINCT m FROM days WHERE m IS NOT NULL) d ORDER BY m");

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

/// A two-column table whose enum gained a member before the first one, so
/// member ids and sort order disagree.
fn reordered_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TYPE tier AS ENUM ('m', 'z')",
            "CREATE TABLE ranked (id int PRIMARY KEY, e tier)",
            "ALTER TYPE tier ADD VALUE 'a' BEFORE 'm'",
            "INSERT INTO ranked VALUES (1, 'z'), (2, 'm'), (3, 'a')",
        ],
    )
}

#[test]
fn should_order_a_limited_read_by_sort_order_not_member_id() {
    // Arrange
    let fixture = reordered_fixture("enum_top_k_ascending");

    // Act
    let rows = fixture.rows("SELECT id, e::text AS label FROM ranked ORDER BY e LIMIT 2");
    let bare = fixture.rows("SELECT id, e FROM ranked ORDER BY e LIMIT 2");

    // Assert
    assert_eq!(
        rows,
        vec![
            vec![Value::Int64(3), Value::String("a".to_string())],
            vec![Value::Int64(2), Value::String("m".to_string())],
        ]
    );
    assert_eq!(
        bare.iter().map(|row| row[0].clone()).collect::<Vec<_>>(),
        vec![Value::Int64(3), Value::Int64(2)]
    );
}

#[test]
fn should_order_a_limited_descending_read_by_sort_order() {
    // Arrange
    let fixture = reordered_fixture("enum_top_k_descending");

    // Act
    let top = fixture.rows("SELECT id, e FROM ranked ORDER BY e DESC LIMIT 1");
    let paged = fixture.rows("SELECT id, e FROM ranked ORDER BY e LIMIT 1 OFFSET 1");

    // Assert
    assert_eq!(
        top.iter().map(|row| row[0].clone()).collect::<Vec<_>>(),
        vec![Value::Int64(1)]
    );
    assert_eq!(
        paged.iter().map(|row| row[0].clone()).collect::<Vec<_>>(),
        vec![Value::Int64(2)]
    );
}

#[test]
fn should_reject_a_set_operation_over_two_enum_types_with_42846() {
    // Arrange
    let fixture = fixture("enum_set_operation_mixed");

    // Act
    let union = fixture.error("SELECT 'ok'::mood UNION SELECT 'sad'::other");
    let intersect = fixture.error("SELECT m FROM days INTERSECT SELECT 'sad'::other");
    let same = fixture.rows("SELECT 'ok'::mood::text UNION SELECT 'ok'::mood::text");

    // Assert
    assert_eq!(union.sql_state(), "42846");
    assert_eq!(intersect.sql_state(), "42846");
    assert_eq!(same.len(), 1);
}

#[test]
fn should_union_two_branches_of_the_same_enum_type() {
    // Arrange
    let fixture = fixture("enum_set_operation_same");

    // Act
    let rows = fixture.rows(
        "SELECT x::text AS label FROM (SELECT 'happy'::mood AS x UNION SELECT 'sad'::mood) u ORDER BY x",
    );

    // Assert
    assert_eq!(texts(&rows), vec!["sad", "happy"]);
}
