//! Table CHECK constraints on enum columns compare members, not label text (#827).

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn fixture(label: &str, setup: &[&str]) -> SqlFixture {
    let mut statements = vec!["CREATE TYPE level AS ENUM ('a', 'b', 'c')"];
    statements.extend_from_slice(setup);
    sql_fixture(label, &statements)
}

fn ids(fixture: &SqlFixture, table: &str) -> Vec<Vec<Value>> {
    fixture.rows(&format!("SELECT id FROM {table} ORDER BY id"))
}

#[test]
fn should_accept_the_member_an_equality_check_names() {
    // Arrange
    let fixture = fixture(
        "enum_check_eq",
        &["CREATE TABLE items (id int, s level CHECK (s = 'a'))"],
    );

    // Act
    let accepted = fixture.execute("INSERT INTO items VALUES (1, 'a')");
    let rejected = fixture.error("INSERT INTO items VALUES (2, 'b')");

    // Assert
    assert!(accepted.is_ok());
    assert_eq!(rejected.sql_state(), "23514");
    assert_eq!(ids(&fixture, "items"), vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_reject_the_member_an_inequality_check_excludes() {
    // Arrange
    let fixture = fixture(
        "enum_check_ne",
        &["CREATE TABLE items (id int, s level CHECK (s <> 'c'))"],
    );

    // Act
    let accepted = fixture.execute("INSERT INTO items VALUES (1, 'b')");
    let rejected = fixture.error("INSERT INTO items VALUES (3, 'c')");

    // Assert
    assert!(accepted.is_ok());
    assert_eq!(rejected.sql_state(), "23514");
    assert_eq!(ids(&fixture, "items"), vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_evaluate_an_ordering_check_by_sort_order() {
    // Arrange
    let fixture = fixture(
        "enum_check_lt",
        &[
            "ALTER TYPE level ADD VALUE 'first' BEFORE 'a'",
            "CREATE TABLE items (id int, s level CHECK (s < 'c'))",
        ],
    );

    // Act
    let first = fixture.execute("INSERT INTO items VALUES (1, 'first')");
    let below = fixture.execute("INSERT INTO items VALUES (2, 'a')");
    let null = fixture.execute("INSERT INTO items VALUES (3, NULL)");
    let at_bound = fixture.error("INSERT INTO items VALUES (4, 'c')");

    // Assert
    assert!(first.is_ok());
    assert!(below.is_ok());
    assert!(null.is_ok());
    assert_eq!(at_bound.sql_state(), "23514");
}

#[test]
fn should_evaluate_a_lower_bound_check_by_sort_order() {
    // Arrange
    let fixture = fixture(
        "enum_check_gte",
        &["CREATE TABLE items (id int, s level CHECK (s >= 'b'))"],
    );

    // Act
    let at_bound = fixture.execute("INSERT INTO items VALUES (1, 'b')");
    let above = fixture.execute("INSERT INTO items VALUES (2, 'c')");
    let below = fixture.error("INSERT INTO items VALUES (3, 'a')");

    // Assert
    assert!(at_bound.is_ok());
    assert!(above.is_ok());
    assert_eq!(below.sql_state(), "23514");
}

#[test]
fn should_enforce_an_enum_check_on_update() {
    // Arrange
    let fixture = fixture(
        "enum_check_update",
        &[
            "CREATE TABLE items (id int, s level CHECK (s <> 'c'))",
            "INSERT INTO items VALUES (1, 'a')",
        ],
    );

    // Act
    let rejected = fixture.error("UPDATE items SET s = 'c' WHERE id = 1");

    // Assert
    assert_eq!(rejected.sql_state(), "23514");
}

#[test]
fn should_validate_existing_rows_when_an_enum_check_is_added() {
    // Arrange
    let fixture = fixture(
        "enum_check_add_constraint",
        &[
            "CREATE TABLE items (id int, s level)",
            "INSERT INTO items VALUES (1, 'c')",
        ],
    );

    // Act
    let violated = fixture.error("ALTER TABLE items ADD CONSTRAINT not_c CHECK (s <> 'c')");
    let satisfied = fixture.execute("ALTER TABLE items ADD CONSTRAINT not_a CHECK (s <> 'a')");

    // Assert
    assert_eq!(violated.sql_state(), "23514");
    assert!(satisfied.is_ok());
}

#[test]
fn should_reject_an_enum_check_on_an_unknown_label_with_22p02() {
    // Arrange
    let fixture = fixture("enum_check_unknown_label", &[]);

    // Act
    let error = fixture.error("CREATE TABLE items (id int, s level CHECK (s = 'zz'))");

    // Assert
    assert_eq!(error.sql_state(), "22P02");
}

#[test]
fn should_keep_an_enum_check_on_its_member_after_rename_value() {
    // Arrange
    let fixture = fixture(
        "enum_check_rename_value",
        &[
            "CREATE TABLE items (id int, s level CHECK (s >= 'b'))",
            "ALTER TYPE level RENAME VALUE 'b' TO 'bee'",
        ],
    );

    // Act
    let renamed = fixture.execute("INSERT INTO items VALUES (1, 'bee')");
    let below = fixture.error("INSERT INTO items VALUES (2, 'a')");

    // Assert
    assert!(renamed.is_ok());
    assert_eq!(below.sql_state(), "23514");
}
