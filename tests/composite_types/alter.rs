//! ALTER TYPE: versioned descriptors, attribute changes and old-revision rows.

use crate::support_composite_types::{first_column_text, only_text};
use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TYPE address AS (street text, zip int)",
            "CREATE TABLE homes (id int, a address)",
            "INSERT INTO homes VALUES (1, ROW('1 Main', 11)), (2, NULL)",
        ],
    )
}

fn text(fixture: &SqlFixture, sql: &str) -> Option<String> {
    only_text(&fixture.rows(sql))
}

#[test]
fn should_read_added_attributes_as_null_for_rows_written_before() {
    // Arrange
    let fixture = fixture("alter_type_add_null");
    fixture
        .execute("ALTER TYPE address ADD ATTRIBUTE city text")
        .unwrap();

    // Act
    let rendered = text(&fixture, "SELECT a::text FROM homes WHERE id = 1");

    // Assert
    assert_eq!(rendered.as_deref(), Some("(\"1 Main\",11,)"));
}

#[test]
fn should_store_added_attributes_for_rows_written_after() {
    // Arrange
    let fixture = fixture("alter_type_add_store");
    fixture
        .execute("ALTER TYPE address ADD ATTRIBUTE city text")
        .unwrap();
    fixture
        .execute("INSERT INTO homes VALUES (3, ROW('2 Elm', 22, 'Oslo'))")
        .unwrap();

    // Act
    let city = text(&fixture, "SELECT (a).city FROM homes WHERE id = 3");

    // Assert
    assert_eq!(city.as_deref(), Some("Oslo"));
}

#[test]
fn should_hide_dropped_attributes_from_rows_written_before() {
    // Arrange
    let fixture = fixture("alter_type_drop_hidden");
    fixture
        .execute("ALTER TYPE address DROP ATTRIBUTE street")
        .unwrap();

    // Act
    let rendered = text(&fixture, "SELECT a::text FROM homes WHERE id = 1");

    // Assert
    assert_eq!(rendered.as_deref(), Some("(11)"));
}

#[test]
fn should_not_resurrect_dropped_data_when_the_attribute_is_added_again() {
    // Arrange
    let fixture = fixture("alter_type_drop_readd");
    fixture
        .execute("ALTER TYPE address DROP ATTRIBUTE street")
        .unwrap();
    fixture
        .execute("ALTER TYPE address ADD ATTRIBUTE street text")
        .unwrap();

    // Act
    let rendered = text(&fixture, "SELECT a::text FROM homes WHERE id = 1");

    // Assert
    assert_eq!(rendered.as_deref(), Some("(11,)"));
}

#[test]
fn should_rename_an_attribute_for_existing_rows() {
    // Arrange
    let fixture = fixture("alter_type_rename_attribute");
    fixture
        .execute("ALTER TYPE address RENAME ATTRIBUTE zip TO postcode")
        .unwrap();

    // Act
    let postcode = text(&fixture, "SELECT (a).postcode FROM homes WHERE id = 1");

    // Assert
    assert_eq!(postcode.as_deref(), Some("11"));
}

#[test]
fn should_keep_rows_readable_after_renaming_the_type() {
    // Arrange
    let fixture = fixture("alter_type_rename_type");
    fixture
        .execute("ALTER TYPE address RENAME TO place")
        .unwrap();

    // Act
    let cast = text(&fixture, "SELECT '(x,1)'::place::text");
    let stored = text(&fixture, "SELECT (a).street FROM homes WHERE id = 1");

    // Assert
    assert_eq!(cast.as_deref(), Some("(x,1)"));
    assert_eq!(stored.as_deref(), Some("1 Main"));
}

#[test]
fn should_stop_resolving_the_old_name_after_a_rename() {
    // Arrange
    let fixture = fixture("alter_type_rename_old_name");
    fixture
        .execute("ALTER TYPE address RENAME TO place")
        .unwrap();

    // Act
    let error = fixture.error("SELECT '(x,1)'::address::text");

    // Assert
    assert_eq!(error.sql_state(), "42704");
}

#[test]
fn should_reject_a_rename_onto_a_table_name_with_sqlstate_42p07() {
    // Arrange
    let fixture = fixture("alter_type_rename_conflict");

    // Act
    let error = fixture.error("ALTER TYPE address RENAME TO homes");

    // Assert
    assert_eq!(error.sql_state(), "42P07");
}

#[test]
fn should_move_a_type_to_another_schema() {
    // Arrange
    let fixture = fixture("alter_type_set_schema");
    fixture.execute("CREATE SCHEMA archive").unwrap();
    fixture
        .execute("ALTER TYPE address SET SCHEMA archive")
        .unwrap();

    // Act
    let cast = text(&fixture, "SELECT '(x,1)'::archive.address::text");

    // Assert
    assert_eq!(cast.as_deref(), Some("(x,1)"));
}

#[test]
fn should_reject_moving_a_type_to_a_missing_schema_with_sqlstate_3f000() {
    // Arrange
    let fixture = fixture("alter_type_set_schema_missing");

    // Act
    let error = fixture.error("ALTER TYPE address SET SCHEMA nowhere");

    // Assert
    assert_eq!(error.sql_state(), "3F000");
}

#[test]
fn should_reject_changing_the_owner_to_a_missing_role_with_sqlstate_42704() {
    // Arrange
    let fixture = fixture("alter_type_owner_missing");

    // Act
    let error = fixture.error("ALTER TYPE address OWNER TO nobody");

    // Assert
    assert_eq!(error.sql_state(), "42704");
}

#[test]
fn should_reject_retyping_an_attribute_stored_in_a_table_with_sqlstate_0a000() {
    // Arrange
    let fixture = fixture("alter_type_retype_stored");

    // Act
    let error = fixture.error("ALTER TYPE address ALTER ATTRIBUTE zip TYPE bigint");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

#[test]
fn should_leave_the_type_unchanged_when_a_retype_is_rejected() {
    // Arrange
    let fixture = fixture("alter_type_retype_unchanged");
    let _ = fixture.error("ALTER TYPE address ALTER ATTRIBUTE zip TYPE bigint");

    // Act
    let rendered = text(&fixture, "SELECT a::text FROM homes WHERE id = 1");

    // Assert
    assert_eq!(rendered.as_deref(), Some("(\"1 Main\",11)"));
}

#[test]
fn should_retype_an_attribute_of_an_unused_type() {
    // Arrange
    let fixture = sql_fixture(
        "alter_type_retype_unused",
        &["CREATE TYPE pair AS (a int, b int)"],
    );
    fixture
        .execute("ALTER TYPE pair ALTER ATTRIBUTE a SET DATA TYPE text")
        .unwrap();

    // Act
    let rendered = text(&fixture, "SELECT ROW('x', 2)::pair::text");

    // Assert
    assert_eq!(rendered.as_deref(), Some("(x,2)"));
}

#[test]
fn should_reject_duplicate_attributes_with_sqlstate_42701() {
    // Arrange
    let fixture = fixture("alter_type_add_duplicate");

    // Act
    let error = fixture.error("ALTER TYPE address ADD ATTRIBUTE zip int");

    // Assert
    assert_eq!(error.sql_state(), "42701");
}

#[test]
fn should_reject_a_missing_attribute_with_sqlstate_42703() {
    // Arrange
    let fixture = fixture("alter_type_drop_missing");

    // Act
    let error = fixture.error("ALTER TYPE address DROP ATTRIBUTE nope");

    // Assert
    assert_eq!(error.sql_state(), "42703");
}

#[test]
fn should_skip_a_missing_attribute_with_if_exists() {
    // Arrange
    let fixture = fixture("alter_type_drop_if_exists");

    // Act
    let outcome = fixture.execute("ALTER TYPE address DROP ATTRIBUTE IF EXISTS nope");

    // Assert
    assert!(outcome.is_ok());
}

#[test]
fn should_reject_a_missing_type_with_sqlstate_42704() {
    // Arrange
    let fixture = fixture("alter_type_missing_type");

    // Act
    let error = fixture.error("ALTER TYPE nope ADD ATTRIBUTE a int");

    // Assert
    assert_eq!(error.sql_state(), "42704");
}

#[test]
fn should_reject_altering_a_table_as_a_type_with_sqlstate_42809() {
    // Arrange
    let fixture = fixture("alter_type_not_composite");

    // Act
    let error = fixture.error("ALTER TYPE homes ADD ATTRIBUTE a int");

    // Assert
    assert_eq!(error.sql_state(), "42809");
}

#[test]
fn should_update_nested_types_when_an_inner_type_gains_an_attribute() {
    // Arrange
    let fixture = sql_fixture(
        "alter_type_nested",
        &[
            "CREATE TYPE inner_t AS (a int)",
            "CREATE TYPE outer_t AS (i inner_t)",
            "CREATE TABLE nest (id int, o outer_t)",
            "INSERT INTO nest VALUES (1, ROW(ROW(5)))",
        ],
    );
    fixture
        .execute("ALTER TYPE inner_t ADD ATTRIBUTE b int")
        .unwrap();

    // Act
    let rendered = text(&fixture, "SELECT o::text FROM nest");

    // Assert
    assert_eq!(rendered.as_deref(), Some("(\"(5,)\")"));
}

#[test]
fn should_publish_new_attributes_in_pg_attribute() {
    // Arrange
    let fixture = fixture("alter_type_catalog");
    fixture
        .execute("ALTER TYPE address ADD ATTRIBUTE city text")
        .unwrap();

    // Act
    let names = first_column_text(&fixture.rows(
        "SELECT attname FROM pg_attribute a JOIN pg_type t ON a.attrelid = t.typrelid \
         WHERE t.typname = 'address' AND a.attnum > 0 ORDER BY a.attnum",
    ));

    // Assert
    assert_eq!(
        names,
        vec![
            Some("street".to_owned()),
            Some("zip".to_owned()),
            Some("city".to_owned())
        ]
    );
}

#[test]
fn should_update_view_columns_when_a_type_gains_an_attribute() {
    // Arrange
    let fixture = fixture("alter_type_view");
    fixture
        .execute("CREATE VIEW home_view AS SELECT a FROM homes")
        .unwrap();
    fixture
        .execute("ALTER TYPE address ADD ATTRIBUTE city text")
        .unwrap();
    fixture
        .execute("INSERT INTO homes VALUES (3, ROW('2 Elm', 22, 'Oslo'))")
        .unwrap();

    // Act
    let city = text(
        &fixture,
        "SELECT (a).city FROM home_view WHERE (a).zip = 22",
    );

    // Assert
    assert_eq!(city.as_deref(), Some("Oslo"));
}

#[test]
fn should_roll_back_add_attribute_with_its_transaction() {
    // Arrange
    let fixture = fixture("alter_type_add_rollback");
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("ALTER TYPE address ADD ATTRIBUTE city text")
        .unwrap();

    // Act
    fixture.execute("ROLLBACK").unwrap();
    let rendered = text(&fixture, "SELECT a::text FROM homes WHERE id = 1");

    // Assert
    assert_eq!(rendered.as_deref(), Some("(\"1 Main\",11)"));
}

#[test]
fn should_report_a_missing_attribute_before_the_stored_use_guard() {
    // Arrange
    let fixture = fixture("alter_type_missing_attribute");

    // Act
    let error = fixture.error("ALTER TYPE address ALTER ATTRIBUTE nope TYPE bigint");

    // Assert
    assert_eq!(error.sql_state(), "42703");
}
