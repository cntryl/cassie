//! Views that depend on a composite attribute: found through other views, and
//! not confused with plain columns that share the attribute's name.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn through_views(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TYPE addr AS (city TEXT, zip TEXT)",
            "CREATE TABLE people (id INT, home addr)",
            "CREATE VIEW pv1 AS SELECT home FROM people",
            "CREATE VIEW pv2 AS SELECT (x.home).city AS c FROM pv1 AS x",
        ],
    )
}

fn with_plain_zip(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TYPE addr2 AS (city TEXT, zip TEXT)",
            "CREATE TABLE ledger (id INT, home addr2, zip TEXT, amount INT)",
            "INSERT INTO ledger VALUES (1, ROW('Oslo', '0150'), 'plain', 7)",
            "CREATE VIEW lv AS SELECT ledger.zip, ledger.amount FROM ledger",
        ],
    )
}

#[test]
fn should_refuse_dropping_an_attribute_read_through_another_view() {
    // Arrange
    let fixture = through_views("composite_drop_through_view");

    // Act
    let error = fixture.error("ALTER TYPE addr DROP ATTRIBUTE city");

    // Assert
    assert_eq!(error.sql_state(), "2BP01");
}

#[test]
fn should_rename_an_attribute_read_through_another_view() {
    // Arrange
    let fixture = through_views("composite_rename_through_view");
    fixture
        .execute("INSERT INTO people VALUES (1, ROW('Oslo', '0150'))")
        .unwrap();
    fixture
        .execute("ALTER TYPE addr RENAME ATTRIBUTE city TO town")
        .unwrap();

    // Act
    let rows = fixture.rows("SELECT c FROM pv2");

    // Assert
    assert_eq!(rows, vec![vec![Value::String("Oslo".into())]]);
}

#[test]
fn should_rename_an_attribute_when_a_plain_column_has_its_name() {
    // Arrange
    let fixture = with_plain_zip("composite_rename_plain_column_name");
    fixture
        .execute("ALTER TYPE addr2 RENAME ATTRIBUTE zip TO postal")
        .unwrap();

    // Act
    let rows = fixture.rows("SELECT zip, amount FROM lv");

    // Assert
    assert_eq!(
        rows,
        vec![vec![Value::String("plain".into()), Value::Int64(7)]]
    );
}

#[test]
fn should_drop_an_attribute_when_a_plain_column_has_its_name() {
    // Arrange
    let fixture = with_plain_zip("composite_drop_plain_column_name");
    fixture
        .execute("ALTER TYPE addr2 DROP ATTRIBUTE zip")
        .unwrap();

    // Act
    let rows = fixture.rows("SELECT zip, amount FROM lv");

    // Assert
    assert_eq!(
        rows,
        vec![vec![Value::String("plain".into()), Value::Int64(7)]]
    );
}

#[test]
fn should_still_refuse_dropping_an_attribute_a_view_selects_from_the_value() {
    // Arrange
    let fixture = with_plain_zip("composite_drop_selected_attribute");
    fixture
        .execute("CREATE VIEW lv2 AS SELECT (home).zip AS z FROM ledger")
        .unwrap();

    // Act
    let error = fixture.error("ALTER TYPE addr2 DROP ATTRIBUTE zip");

    // Assert
    assert_eq!(error.sql_state(), "2BP01");
}
