//! Scalar indexes on enum columns are keyed by member ID: equality and IN probes
//! survive RENAME VALUE and ADD VALUE, while ranges and ORDER BY follow sort order.
//! Authored against the v0.4.0 contract.

use crate::support_enum_types::{scalar, strings};
use crate::support_sql_fixture::{sql_fixture, SqlFixture};

const SETUP: [&str; 3] = [
    "CREATE TYPE priority AS ENUM ('low', 'medium', 'high')",
    "CREATE TABLE tasks (id int PRIMARY KEY, p priority)",
    "INSERT INTO tasks VALUES (1, 'medium'), (2, 'high'), (3, NULL), (4, 'low'), (5, 'high'), (6, 'low')",
];

fn indexed(label: &str) -> SqlFixture {
    let fixture = sql_fixture(label, &SETUP);
    fixture
        .execute("CREATE INDEX tasks_priority ON tasks (p)")
        .unwrap();
    fixture
}

fn ids(fixture: &SqlFixture, predicate: &str) -> Vec<String> {
    strings(
        fixture,
        &format!("SELECT id::text FROM tasks WHERE {predicate} ORDER BY id"),
    )
}

#[test]
fn should_probe_equality_through_an_index_built_over_existing_rows() {
    // Arrange
    let fixture = indexed("enum_index_backfill");

    // Act
    let high = ids(&fixture, "p = 'high'");
    let low = ids(&fixture, "p = 'low'::priority");
    let absent = ids(&fixture, "p = 'medium' AND id > 1");

    // Assert
    assert_eq!(high, vec!["2", "5"]);
    assert_eq!(low, vec!["4", "6"]);
    assert!(absent.is_empty());
}

#[test]
fn should_maintain_an_index_created_before_the_rows() {
    // Arrange
    let fixture = sql_fixture(
        "enum_index_maintained",
        &[
            SETUP[0],
            SETUP[1],
            "CREATE INDEX tasks_priority ON tasks (p)",
        ],
    );

    // Act
    fixture
        .execute("INSERT INTO tasks VALUES (1, 'high'), (2, 'low'), (3, 'high')")
        .unwrap();
    fixture
        .execute("UPDATE tasks SET p = 'low' WHERE id = 3")
        .unwrap();
    fixture.execute("DELETE FROM tasks WHERE id = 2").unwrap();

    // Assert
    assert_eq!(ids(&fixture, "p = 'high'"), vec!["1"]);
    assert_eq!(ids(&fixture, "p = 'low'"), vec!["3"]);
}

#[test]
fn should_probe_in_and_any_lists_through_the_index() {
    // Arrange
    let fixture = indexed("enum_index_in_any");

    // Act
    let in_list = ids(&fixture, "p IN ('low', 'medium')");
    let any_array = ids(&fixture, "p = ANY (ARRAY['high','medium']::priority[])");
    let any_literal = ids(&fixture, "p = ANY('{low,high}')");

    // Assert
    assert_eq!(in_list, vec!["1", "4", "6"]);
    assert_eq!(any_array, vec!["1", "2", "5"]);
    assert_eq!(any_literal, vec!["2", "4", "5", "6"]);
}

#[test]
fn should_find_rows_by_the_new_label_after_rename_value() {
    // Arrange
    let fixture = indexed("enum_index_rename_value");

    // Act
    fixture
        .execute("ALTER TYPE priority RENAME VALUE 'high' TO 'critical'")
        .unwrap();
    let renamed = ids(&fixture, "p = 'critical'");
    let in_list = ids(&fixture, "p IN ('critical', 'low')");
    let old_label = fixture.error("SELECT id FROM tasks WHERE p = 'high'");
    fixture
        .execute("INSERT INTO tasks VALUES (7, 'critical')")
        .unwrap();
    let after_insert = ids(&fixture, "p = 'critical'");

    // Assert
    assert_eq!(renamed, vec!["2", "5"], "the index key is the member ID");
    assert_eq!(in_list, vec!["2", "4", "5", "6"]);
    assert_eq!(old_label.sql_state(), "22P02");
    assert_eq!(after_insert, vec!["2", "5", "7"]);
}

#[test]
fn should_keep_equality_probes_correct_after_add_value_before() {
    // Arrange
    let fixture = indexed("enum_index_add_before");

    // Act
    fixture
        .execute("ALTER TYPE priority ADD VALUE 'minor' BEFORE 'medium'")
        .unwrap();
    fixture
        .execute("INSERT INTO tasks VALUES (7, 'minor'), (8, 'minor')")
        .unwrap();
    let minor = ids(&fixture, "p = 'minor'");
    let medium = ids(&fixture, "p = 'medium'");
    let high = ids(&fixture, "p = 'high'");

    // Assert
    assert_eq!(minor, vec!["7", "8"]);
    assert_eq!(medium, vec!["1"]);
    assert_eq!(high, vec!["2", "5"]);
}

#[test]
fn should_answer_range_predicates_by_sort_order_with_the_index_present() {
    // Arrange
    let fixture = indexed("enum_index_range");

    // Act
    let above_low = ids(&fixture, "p > 'low'");
    let up_to_medium = ids(&fixture, "p <= 'medium'");
    let between = ids(&fixture, "p BETWEEN 'medium' AND 'high'");

    // Assert
    assert_eq!(above_low, vec!["1", "2", "5"]);
    assert_eq!(up_to_medium, vec!["1", "4", "6"]);
    assert_eq!(between, vec!["1", "2", "5"]);
}

#[test]
fn should_answer_range_predicates_by_sort_order_after_add_value_before() {
    // Arrange
    let fixture = indexed("enum_index_range_after_add");
    fixture
        .execute("ALTER TYPE priority ADD VALUE 'minor' BEFORE 'low'")
        .unwrap();
    fixture
        .execute("INSERT INTO tasks VALUES (7, 'minor')")
        .unwrap();

    // Act
    let below_low = ids(&fixture, "p < 'low'");
    let at_most_low = ids(&fixture, "p <= 'low'");
    let above_minor = ids(&fixture, "p > 'minor'");

    // Assert
    assert_eq!(below_low, vec!["7"], "a later-added member sorts first");
    assert_eq!(at_most_low, vec!["4", "6", "7"]);
    assert_eq!(above_minor, vec!["1", "2", "4", "5", "6"]);
}

#[test]
fn should_order_by_declared_order_with_the_index_present() {
    // Arrange
    let fixture = indexed("enum_index_order_by");
    fixture
        .execute("ALTER TYPE priority ADD VALUE 'minor' BEFORE 'low'")
        .unwrap();
    fixture
        .execute("INSERT INTO tasks VALUES (7, 'minor')")
        .unwrap();

    // Act
    let ascending = strings(&fixture, "SELECT id::text FROM tasks ORDER BY p, id");
    let descending = strings(&fixture, "SELECT id::text FROM tasks ORDER BY p DESC, id");
    let limited = strings(
        &fixture,
        "SELECT id::text FROM tasks WHERE p IS NOT NULL ORDER BY p, id LIMIT 3",
    );

    // Assert
    assert_eq!(ascending, vec!["7", "4", "6", "1", "2", "5", "3"]);
    assert_eq!(descending, vec!["3", "2", "5", "1", "4", "6", "7"]);
    assert_eq!(limited, vec!["7", "4", "6"]);
}

#[test]
fn should_reject_a_duplicate_key_in_a_unique_index_with_sqlstate_23505() {
    // Arrange
    let fixture = sql_fixture(
        "enum_index_unique",
        &[
            SETUP[0],
            "CREATE TABLE slots (id int PRIMARY KEY, p priority)",
            "CREATE UNIQUE INDEX slots_priority ON slots (p)",
            "INSERT INTO slots VALUES (1, 'low'), (2, 'high')",
        ],
    );

    // Act
    let duplicate = fixture.error("INSERT INTO slots VALUES (3, 'low')");
    fixture
        .execute("ALTER TYPE priority RENAME VALUE 'low' TO 'minimal'")
        .unwrap();
    let after_rename = fixture.error("INSERT INTO slots VALUES (4, 'minimal')");
    let distinct = fixture.execute("INSERT INTO slots VALUES (5, 'medium')");

    // Assert
    assert_eq!(duplicate.sql_state(), "23505");
    assert_eq!(after_rename.sql_state(), "23505");
    assert!(distinct.is_ok(), "a distinct member is accepted");
}

#[test]
fn should_count_rows_per_member_after_rename_value_and_add_value() {
    // Arrange
    let fixture = indexed("enum_index_counts");
    fixture
        .execute("ALTER TYPE priority RENAME VALUE 'medium' TO 'normal'")
        .unwrap();
    fixture
        .execute("ALTER TYPE priority ADD VALUE 'urgent'")
        .unwrap();
    fixture
        .execute("INSERT INTO tasks VALUES (7, 'urgent'), (8, 'normal')")
        .unwrap();

    // Act
    let normal = scalar(
        &fixture,
        "SELECT count(*)::text FROM tasks WHERE p = 'normal'",
    );
    let urgent = scalar(
        &fixture,
        "SELECT count(*)::text FROM tasks WHERE p = 'urgent'",
    );
    let total = scalar(
        &fixture,
        "SELECT count(*)::text FROM tasks WHERE p IS NOT NULL",
    );

    // Assert
    assert_eq!(normal.as_deref(), Some("2"));
    assert_eq!(urgent.as_deref(), Some("1"));
    assert_eq!(total.as_deref(), Some("7"));
}
