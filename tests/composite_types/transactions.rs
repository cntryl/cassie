//! Records written and then re-described inside one transaction.

use crate::support_composite_types::only_text;
use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_commit_a_record_written_before_its_type_gained_an_attribute() {
    // Arrange
    let fixture = sql_fixture(
        "composite_commit_after_add_attribute",
        &[
            "CREATE TYPE md AS (a int)",
            "CREATE TABLE mdt (id int primary key, v md)",
        ],
    );
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("INSERT INTO mdt VALUES (1, ROW(1))")
        .unwrap();
    fixture
        .execute("ALTER TYPE md ADD ATTRIBUTE b int")
        .unwrap();

    // Act
    fixture.execute("COMMIT").unwrap();
    let stored = only_text(&fixture.rows("SELECT v::text FROM mdt WHERE id = 1"));

    // Assert
    assert_eq!(stored.as_deref(), Some("(1,)"));
}

#[test]
fn should_commit_a_row_value_written_before_its_table_gained_a_column() {
    // Arrange
    let fixture = sql_fixture(
        "composite_commit_after_add_column",
        &[
            "CREATE TABLE rb (a int, b int)",
            "CREATE TABLE rh (id int primary key, r rb)",
        ],
    );
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("INSERT INTO rh VALUES (1, ROW(1, 1))")
        .unwrap();
    fixture.execute("ALTER TABLE rb ADD COLUMN z int").unwrap();

    // Act
    fixture.execute("COMMIT").unwrap();
    let stored = only_text(&fixture.rows("SELECT r::text FROM rh WHERE id = 1"));

    // Assert
    assert_eq!(stored.as_deref(), Some("(1,1,)"));
}
