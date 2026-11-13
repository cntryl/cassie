//! Dependency rules of composite and table row types: cycles, views that bind
//! a type by name, and the table that owns a row type.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn cycles(label: &str) -> SqlFixture {
    sql_fixture(label, &["CREATE TABLE cyc_a (id int primary key, x int)"])
}

fn viewed(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TYPE vt AS (a int, b text)",
            "CREATE TABLE vtt (id int primary key, v vt)",
            "CREATE VIEW vt_cast AS SELECT ROW(1, 'x')::vt AS r",
            "CREATE VIEW vt_field AS SELECT id, (v).b AS b FROM vtt",
        ],
    )
}

#[test]
fn should_reject_a_column_of_the_tables_own_row_type_with_sqlstate_42p16() {
    // Arrange
    let fixture = cycles("row_type_self_member");

    // Act
    let error = fixture.error("ALTER TABLE cyc_a ADD COLUMN self cyc_a");

    // Assert
    assert_eq!(error.sql_state(), "42P16");
}

#[test]
fn should_reject_a_row_type_cycle_through_a_composite_type_with_sqlstate_42p16() {
    // Arrange
    let fixture = cycles("row_type_cycle_through_type");
    fixture.execute("CREATE TYPE cyc_c AS (r cyc_a)").unwrap();

    // Act
    let error = fixture.error("ALTER TABLE cyc_a ADD COLUMN y cyc_c");

    // Assert
    assert_eq!(error.sql_state(), "42P16");
}

#[test]
fn should_reject_a_row_type_cycle_through_another_table_with_sqlstate_42p16() {
    // Arrange
    let fixture = cycles("row_type_cycle_through_table");
    fixture
        .execute("CREATE TABLE cyc_b (id int primary key, a cyc_a)")
        .unwrap();

    // Act
    let error = fixture.error("ALTER TABLE cyc_a ADD COLUMN b cyc_b");

    // Assert
    assert_eq!(error.sql_state(), "42P16");
}

#[test]
fn should_keep_a_table_droppable_after_a_rejected_self_member() {
    // Arrange
    let fixture = cycles("row_type_self_member_drop");
    let _rejected = fixture.error("ALTER TABLE cyc_a ADD COLUMN self cyc_a");

    // Act
    let dropped = fixture.execute("DROP TABLE cyc_a");

    // Assert
    assert!(dropped.is_ok());
}

#[test]
fn should_refuse_to_rename_a_type_a_view_names_with_sqlstate_0a000() {
    // Arrange
    let fixture = viewed("alter_type_rename_under_view");

    // Act
    let error = fixture.error("ALTER TYPE vt RENAME TO vt2");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

#[test]
fn should_keep_a_view_working_after_a_refused_type_rename() {
    // Arrange
    let fixture = viewed("alter_type_rename_view_survives");
    let _refused = fixture.error("ALTER TYPE vt RENAME TO vt2");

    // Act
    let rows = fixture.rows("SELECT r::text FROM vt_cast");

    // Assert
    assert_eq!(rows.len(), 1);
}

#[test]
fn should_refuse_to_rename_an_attribute_a_view_selects_with_sqlstate_0a000() {
    // Arrange
    let fixture = viewed("alter_type_rename_attribute_under_view");

    // Act
    let error = fixture.error("ALTER TYPE vt RENAME ATTRIBUTE b TO bb");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

#[test]
fn should_refuse_to_drop_an_attribute_a_view_selects_with_sqlstate_2bp01() {
    // Arrange
    let fixture = viewed("alter_type_drop_attribute_under_view");

    // Act
    let error = fixture.error("ALTER TYPE vt DROP ATTRIBUTE b");

    // Assert
    assert_eq!(error.sql_state(), "2BP01");
}

#[test]
fn should_rename_an_attribute_no_view_selects() {
    // Arrange
    let fixture = viewed("alter_type_rename_unviewed_attribute");

    // Act
    let renamed = fixture.execute("ALTER TYPE vt RENAME ATTRIBUTE a TO aa");

    // Assert
    assert!(renamed.is_ok());
}

#[test]
fn should_reject_renaming_an_attribute_to_itself_with_sqlstate_42701() {
    // Arrange
    let fixture = viewed("alter_type_rename_attribute_to_itself");

    // Act
    let error = fixture.error("ALTER TYPE vt RENAME ATTRIBUTE a TO a");

    // Assert
    assert_eq!(error.sql_state(), "42701");
}

#[test]
fn should_reject_dropping_a_table_row_type_with_sqlstate_2bp01() {
    // Arrange
    let fixture = viewed("drop_type_of_table_row_type");

    // Act
    let error = fixture.error("DROP TYPE vtt");

    // Assert
    assert_eq!(error.sql_state(), "2BP01");
}
