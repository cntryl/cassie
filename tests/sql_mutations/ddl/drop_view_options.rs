//! `DROP VIEW` drop behavior keywords are options, never part of the view name.

use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn fixture() -> SqlFixture {
    sql_fixture(
        "drop-view-options",
        &[
            "CREATE TABLE drop_view_source (id INT PRIMARY KEY)",
            "CREATE VIEW drop_view_first AS SELECT id FROM drop_view_source",
            "CREATE VIEW drop_view_second AS SELECT id FROM drop_view_source",
        ],
    )
}

fn view_count(fixture: &SqlFixture) -> Vec<Vec<Value>> {
    fixture.rows(
        "SELECT count(*) FROM pg_class WHERE relname IN ('drop_view_first', 'drop_view_second')",
    )
}

#[test]
fn should_drop_a_view_list_given_the_default_restrict_option() {
    // Arrange
    let fixture = fixture();

    // Act
    let dropped = fixture.execute("DROP VIEW IF EXISTS drop_view_first, drop_view_second RESTRICT");
    let remaining = view_count(&fixture);

    // Assert
    assert!(dropped.is_ok(), "RESTRICT is accepted after a name list");
    assert_eq!(remaining, vec![vec![Value::Int64(0)]]);
}

#[test]
fn should_drop_a_view_list_given_the_cascade_option() {
    // Arrange
    let fixture = fixture();

    // Act
    let dropped = fixture.execute("DROP VIEW drop_view_first, drop_view_second CASCADE");
    let remaining = view_count(&fixture);

    // Assert
    assert!(dropped.is_ok(), "CASCADE is an option after a name list");
    assert_eq!(remaining, vec![vec![Value::Int64(0)]]);
}
