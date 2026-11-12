//! CREATE TYPE and DROP TYPE lifecycle, errors and transactional publication.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

const ADDRESS: &str = "CREATE TYPE address AS (street text, zip int)";

fn fixture_with_address(label: &str) -> SqlFixture {
    sql_fixture(label, &[ADDRESS])
}

#[test]
fn should_create_and_drop_a_composite_type() {
    // Arrange
    let fixture = sql_fixture("composite_create_drop", &[]);

    // Act
    let created = fixture.execute(ADDRESS);
    let usable = fixture.execute("SELECT '(a,1)'::address::text");
    let dropped = fixture.execute("DROP TYPE address");
    let after_drop = fixture.execute("SELECT '(a,1)'::address::text");

    // Assert
    assert!(created.is_ok(), "CREATE TYPE must succeed");
    assert!(usable.is_ok(), "created type must be usable in casts");
    assert!(dropped.is_ok(), "DROP TYPE must succeed");
    assert!(after_drop.is_err(), "dropped type must no longer resolve");
}

#[test]
fn should_reject_a_duplicate_type_with_sqlstate_42710() {
    // Arrange
    let fixture = fixture_with_address("composite_duplicate");

    // Act
    let error = fixture.error(ADDRESS);

    // Assert
    assert_eq!(error.sql_state(), "42710");
}

#[test]
fn should_reject_duplicate_attribute_names_with_sqlstate_42701() {
    // Arrange
    let fixture = sql_fixture("composite_duplicate_attribute", &[]);

    // Act
    let error = fixture.error("CREATE TYPE pair AS (a int, a text)");

    // Assert
    assert_eq!(error.sql_state(), "42701");
}

#[test]
fn should_create_a_type_in_an_explicit_schema_and_resolve_it_through_search_path() {
    // Arrange
    let fixture = sql_fixture("composite_schema", &["CREATE SCHEMA app"]);

    // Act
    let created = fixture.execute("CREATE TYPE app.pair AS (a int, b text)");
    let qualified = fixture.execute("SELECT '(1,x)'::app.pair::text");
    let unqualified_before = fixture.execute("SELECT '(1,x)'::pair::text");
    fixture.execute("SET search_path TO app, public").unwrap();
    let unqualified_after = fixture.execute("SELECT '(1,x)'::pair::text");

    // Assert
    assert!(created.is_ok(), "schema-qualified CREATE TYPE must succeed");
    assert!(qualified.is_ok(), "qualified name must resolve");
    assert!(
        unqualified_before.is_err(),
        "type outside search_path must not resolve"
    );
    assert!(unqualified_after.is_ok(), "search_path must expose the type");
}

#[test]
fn should_reject_a_type_in_a_missing_schema_with_sqlstate_3f000() {
    // Arrange
    let fixture = sql_fixture("composite_missing_schema", &[]);

    // Act
    let error = fixture.error("CREATE TYPE nowhere.pair AS (a int)");

    // Assert
    assert_eq!(error.sql_state(), "3F000");
}

#[test]
fn should_support_nested_composite_attributes() {
    // Arrange
    let fixture = fixture_with_address("composite_nested_type");

    // Act
    let created = fixture.execute("CREATE TYPE person AS (name text, home address)");
    let cast = fixture.execute("SELECT '(Ann,\"(\"\"1 Main\"\",7)\")'::person::text");

    // Assert
    assert!(created.is_ok(), "nested attribute must be accepted");
    assert_eq!(
        crate::support_composite_types::only_text(cast.unwrap().rows()),
        Some("(Ann,\"(\"\"1 Main\"\",7)\")".to_owned())
    );
}

#[test]
fn should_reject_drop_with_dependent_column_with_sqlstate_2bp01() {
    // Arrange
    let fixture = fixture_with_address("composite_drop_column_dependent");
    fixture
        .execute("CREATE TABLE homes (id int, a address)")
        .unwrap();

    // Act
    let error = fixture.error("DROP TYPE address");
    let restrict = fixture.error("DROP TYPE address RESTRICT");

    // Assert
    assert_eq!(error.sql_state(), "2BP01");
    assert_eq!(restrict.sql_state(), "2BP01");
}

#[test]
fn should_reject_drop_with_dependent_composite_type_with_sqlstate_2bp01() {
    // Arrange
    let fixture = fixture_with_address("composite_drop_type_dependent");
    fixture
        .execute("CREATE TYPE person AS (name text, home address)")
        .unwrap();

    // Act
    let error = fixture.error("DROP TYPE address");

    // Assert
    assert_eq!(error.sql_state(), "2BP01");
}

#[test]
fn should_allow_drop_after_the_dependent_table_is_dropped() {
    // Arrange
    let fixture = fixture_with_address("composite_drop_after_table");
    fixture
        .execute("CREATE TABLE homes (id int, a address)")
        .unwrap();
    fixture.execute("DROP TABLE homes").unwrap();

    // Act
    let dropped = fixture.execute("DROP TYPE address");

    // Assert
    assert!(dropped.is_ok(), "no dependents remain");
}

#[test]
fn should_honor_drop_type_if_exists_and_report_missing_types() {
    // Arrange
    let fixture = sql_fixture("composite_drop_missing", &[]);

    // Act
    let quiet = fixture.execute("DROP TYPE IF EXISTS never_created");
    let error = fixture.error("DROP TYPE never_created");

    // Assert
    assert!(quiet.is_ok(), "IF EXISTS must not fail");
    assert_eq!(error.sql_state(), "42704");
}

#[test]
fn should_keep_deterministic_unsupported_errors_for_excluded_type_forms() {
    // Arrange
    let fixture = sql_fixture("composite_excluded_forms", &[]);

    // Act
    let enum_error = fixture.error("CREATE TYPE mood AS ENUM ('sad', 'ok')");
    let range_error = fixture.error("CREATE TYPE span AS RANGE (subtype = int)");

    // Assert
    assert_eq!(enum_error.sql_state(), "0A000");
    assert_eq!(range_error.sql_state(), "0A000");
}

#[test]
fn should_roll_back_create_type_with_its_transaction() {
    // Arrange
    let fixture = sql_fixture("composite_create_rollback", &[]);
    fixture.execute("BEGIN").unwrap();
    fixture.execute(ADDRESS).unwrap();
    let visible_inside = fixture.execute("SELECT '(a,1)'::address::text");

    // Act
    fixture.execute("ROLLBACK").unwrap();
    let after_rollback = fixture.execute("SELECT '(a,1)'::address::text");
    let recreated = fixture.execute(ADDRESS);

    // Assert
    assert!(visible_inside.is_ok(), "type is visible inside its transaction");
    assert!(after_rollback.is_err(), "rolled-back type must not survive");
    assert!(recreated.is_ok(), "rolled-back name must be reusable");
}

#[test]
fn should_publish_create_type_on_commit() {
    // Arrange
    let fixture = sql_fixture("composite_create_commit", &[]);
    fixture.execute("BEGIN").unwrap();
    fixture.execute(ADDRESS).unwrap();

    // Act
    fixture.execute("COMMIT").unwrap();
    let after_commit = fixture.execute("SELECT '(a,1)'::address::text");

    // Assert
    assert!(after_commit.is_ok(), "committed type must be visible");
}

#[test]
fn should_roll_back_drop_type_with_its_transaction() {
    // Arrange
    let fixture = fixture_with_address("composite_drop_rollback");
    fixture.execute("BEGIN").unwrap();
    fixture.execute("DROP TYPE address").unwrap();

    // Act
    fixture.execute("ROLLBACK").unwrap();
    let after_rollback = fixture.execute("SELECT '(a,1)'::address::text");

    // Assert
    assert!(after_rollback.is_ok(), "rolled-back drop must keep the type");
}

#[test]
fn should_reject_dropping_a_schema_that_still_owns_a_type() {
    // Arrange
    let fixture = sql_fixture("composite_schema_owns_type", &["CREATE SCHEMA app"]);
    fixture
        .execute("CREATE TYPE app.pair AS (a int, b text)")
        .unwrap();

    // Act
    let restricted = fixture.execute("DROP SCHEMA app");
    fixture.execute("DROP TYPE app.pair").unwrap();
    let emptied = fixture.execute("DROP SCHEMA app");

    // Assert
    assert!(restricted.is_err(), "schema with a type is not empty");
    assert!(emptied.is_ok(), "schema is empty after the type is dropped");
}
