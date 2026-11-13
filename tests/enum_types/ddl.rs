//! CREATE TYPE ... AS ENUM and DROP TYPE lifecycle and errors (#827 contract).
//! Authored against the PostgreSQL 18.6 oracle.

use crate::support_enum_types::{scalar, strings};
use crate::support_sql_fixture::{sql_fixture, SqlFixture};

const STATE: &str = "CREATE TYPE state_code AS ENUM ('open', 'closed')";

fn fixture_with_state(label: &str) -> SqlFixture {
    sql_fixture(label, &[STATE])
}

#[test]
fn should_make_a_created_enum_usable_in_casts() {
    // Arrange
    let fixture = sql_fixture("enum_create", &[]);

    // Act
    let created = fixture.execute(STATE);
    let rendered = scalar(&fixture, "SELECT 'open'::state_code::text");

    // Assert
    assert!(created.is_ok(), "CREATE TYPE ... AS ENUM must succeed");
    assert_eq!(rendered.as_deref(), Some("open"));
}

#[test]
fn should_create_an_enum_with_no_labels() {
    // Arrange
    let fixture = sql_fixture("enum_create_empty", &[]);

    // Act
    let created = fixture.execute("CREATE TYPE nothing AS ENUM ()");
    let range = scalar(&fixture, "SELECT enum_range(NULL::nothing)::text");

    // Assert
    assert!(created.is_ok(), "an empty label list is accepted");
    assert_eq!(range.as_deref(), Some("{}"));
}

#[test]
fn should_create_an_enum_in_a_schema_resolved_through_search_path() {
    // Arrange
    let fixture = sql_fixture("enum_schema", &["CREATE SCHEMA app"]);

    // Act
    let created = fixture.execute("CREATE TYPE app.state_code AS ENUM ('open', 'closed')");
    let qualified = fixture.execute("SELECT 'open'::app.state_code::text");
    let unqualified_before = fixture.execute("SELECT 'open'::state_code::text");
    fixture.execute("SET search_path TO app, public").unwrap();
    let unqualified_after = scalar(&fixture, "SELECT 'open'::state_code::text");

    // Assert
    assert!(created.is_ok(), "schema-qualified CREATE TYPE must succeed");
    assert!(qualified.is_ok(), "qualified name must resolve");
    assert!(
        unqualified_before.is_err(),
        "an enum outside search_path must not resolve unqualified"
    );
    assert_eq!(unqualified_after.as_deref(), Some("open"));
}

#[test]
fn should_reject_a_duplicate_label_with_sqlstate_23505() {
    // Arrange
    let fixture = sql_fixture("enum_duplicate_label", &[]);

    // Act
    let error = fixture.error("CREATE TYPE dup AS ENUM ('a', 'a')");
    let absent = fixture.execute("SELECT 'a'::dup::text");

    // Assert
    assert_eq!(error.sql_state(), "23505");
    assert!(absent.is_err(), "the failed CREATE must leave no type");
}

#[test]
fn should_accept_a_label_of_exactly_63_bytes() {
    // Arrange
    let fixture = sql_fixture("enum_label_63", &[]);
    let label = "a".repeat(63);

    // Act
    let created = fixture.execute(&format!("CREATE TYPE wide AS ENUM ('{label}')"));
    let length = scalar(
        &fixture,
        &format!("SELECT length('{label}'::wide::text)::text"),
    );

    // Assert
    assert!(created.is_ok(), "63 bytes is the longest label");
    assert_eq!(length.as_deref(), Some("63"));
}

#[test]
fn should_reject_a_label_of_64_bytes_with_sqlstate_42602() {
    // Arrange
    let fixture = sql_fixture("enum_label_64", &[]);
    let label = "a".repeat(64);

    // Act
    let error = fixture.error(&format!("CREATE TYPE wide AS ENUM ('{label}')"));

    // Assert
    assert_eq!(error.sql_state(), "42602");
}

#[test]
fn should_measure_label_length_in_bytes_not_characters() {
    // Arrange
    let fixture = sql_fixture("enum_label_multibyte", &[]);
    let thirty_one = "\u{e9}".repeat(31); // 62 bytes
    let thirty_two = "\u{e9}".repeat(32); // 64 bytes

    // Act
    let accepted = fixture.execute(&format!("CREATE TYPE fits AS ENUM ('{thirty_one}')"));
    let error = fixture.error(&format!("CREATE TYPE spills AS ENUM ('{thirty_two}')"));

    // Assert
    assert!(accepted.is_ok(), "62 bytes fits");
    assert_eq!(error.sql_state(), "42602");
}

#[test]
fn should_reject_a_name_used_by_a_table_with_sqlstate_42710() {
    // Arrange
    let fixture = sql_fixture("enum_collision_table", &["CREATE TABLE taken (a int)"]);

    // Act
    let error = fixture.error("CREATE TYPE taken AS ENUM ('x')");

    // Assert
    assert_eq!(error.sql_state(), "42710");
}

#[test]
fn should_reject_a_table_named_like_an_existing_enum_with_sqlstate_42710() {
    // Arrange
    let fixture = fixture_with_state("enum_collision_enum_first");

    // Act
    let error = fixture.error("CREATE TABLE state_code (a int)");

    // Assert
    assert_eq!(error.sql_state(), "42710");
}

#[test]
fn should_reject_a_name_used_by_a_composite_type_with_sqlstate_42710() {
    // Arrange
    let fixture = sql_fixture(
        "enum_collision_composite",
        &["CREATE TYPE pair AS (a int, b text)"],
    );

    // Act
    let error = fixture.error("CREATE TYPE pair AS ENUM ('x')");
    let reverse = fixture_with_state("enum_collision_composite_reverse")
        .error("CREATE TYPE state_code AS (a int)")
        .sql_state();

    // Assert
    assert_eq!(error.sql_state(), "42710");
    assert_eq!(reverse, "42710");
}

#[test]
fn should_reject_creating_the_same_enum_twice_with_sqlstate_42710() {
    // Arrange
    let fixture = fixture_with_state("enum_duplicate_type");

    // Act
    let error = fixture.error(STATE);

    // Assert
    assert_eq!(error.sql_state(), "42710");
}

#[test]
fn should_stop_resolving_a_dropped_enum() {
    // Arrange
    let fixture = fixture_with_state("enum_drop");

    // Act
    let dropped = fixture.execute("DROP TYPE state_code");
    let after_drop = fixture.execute("SELECT 'open'::state_code::text");

    // Assert
    assert!(dropped.is_ok(), "DROP TYPE must succeed");
    assert!(after_drop.is_err(), "a dropped enum must no longer resolve");
}

#[test]
fn should_drop_several_enums_in_one_statement() {
    // Arrange
    let fixture = sql_fixture(
        "enum_drop_many",
        &[
            "CREATE TYPE first_kind AS ENUM ('p')",
            "CREATE TYPE second_kind AS ENUM ('p')",
        ],
    );

    // Act
    let dropped = fixture.execute("DROP TYPE first_kind, second_kind");
    let first = fixture.execute("SELECT 'p'::first_kind::text");
    let second = fixture.execute("SELECT 'p'::second_kind::text");

    // Assert
    assert!(dropped.is_ok());
    assert!(first.is_err() && second.is_err());
}

#[test]
fn should_honor_drop_type_if_exists_for_a_missing_enum() {
    // Arrange
    let fixture = sql_fixture("enum_drop_missing", &[]);

    // Act
    let quiet = fixture.execute("DROP TYPE IF EXISTS never_created");
    let error = fixture.error("DROP TYPE never_created");

    // Assert
    assert!(quiet.is_ok(), "IF EXISTS must not fail");
    assert_eq!(error.sql_state(), "42704");
}

#[test]
fn should_reject_drop_with_a_dependent_column_with_sqlstate_2bp01() {
    // Arrange
    let fixture = fixture_with_state("enum_drop_restrict");
    fixture
        .execute("CREATE TABLE jobs (id int, st state_code)")
        .unwrap();

    // Act
    let implicit = fixture.error("DROP TYPE state_code");
    let explicit = fixture.error("DROP TYPE state_code RESTRICT");
    let still_usable = scalar(&fixture, "SELECT 'open'::state_code::text");

    // Assert
    assert_eq!(implicit.sql_state(), "2BP01");
    assert_eq!(explicit.sql_state(), "2BP01");
    assert_eq!(still_usable.as_deref(), Some("open"));
}

#[test]
fn should_reject_drop_with_a_dependent_array_column_with_sqlstate_2bp01() {
    // Arrange
    let fixture = fixture_with_state("enum_drop_array_dependent");
    fixture
        .execute("CREATE TABLE jobs (id int, sts state_code[])")
        .unwrap();

    // Act
    let error = fixture.error("DROP TYPE state_code");

    // Assert
    assert_eq!(error.sql_state(), "2BP01");
}

#[test]
fn should_allow_drop_once_the_dependent_table_is_dropped() {
    // Arrange
    let fixture = fixture_with_state("enum_drop_after_table");
    fixture
        .execute("CREATE TABLE jobs (id int, st state_code)")
        .unwrap();
    fixture.execute("DROP TABLE jobs").unwrap();

    // Act
    let dropped = fixture.execute("DROP TYPE state_code");

    // Assert
    assert!(dropped.is_ok(), "no dependents remain");
}

#[test]
fn should_reject_drop_cascade_with_dependents_with_sqlstate_0a000() {
    // Arrange
    let fixture = fixture_with_state("enum_drop_cascade");
    fixture
        .execute("CREATE TABLE jobs (id int, st state_code)")
        .unwrap();

    // Act
    let error = fixture.error("DROP TYPE state_code CASCADE");
    let still_usable = scalar(&fixture, "SELECT 'closed'::state_code::text");
    let columns = strings(
        &fixture,
        "SELECT column_name FROM information_schema.columns WHERE table_name = 'jobs' \
         ORDER BY ordinal_position",
    );

    // Assert
    assert_eq!(error.sql_state(), "0A000");
    assert_eq!(still_usable.as_deref(), Some("closed"));
    assert_eq!(
        columns,
        vec!["id", "st"],
        "CASCADE must not drop the column"
    );
}

#[test]
fn should_accept_drop_cascade_without_dependents() {
    // Arrange
    let fixture = fixture_with_state("enum_drop_cascade_free");

    // Act
    let dropped = fixture.execute("DROP TYPE state_code CASCADE");

    // Assert
    assert!(
        dropped.is_ok(),
        "CASCADE with no dependents is a plain drop"
    );
}

#[test]
fn should_reject_an_enum_field_in_a_composite_type_with_sqlstate_0a000() {
    // Arrange
    let fixture = fixture_with_state("enum_composite_field");

    // Act
    let error = fixture.error("CREATE TYPE job AS (id int, st state_code)");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

#[test]
fn should_reject_an_enum_as_a_domain_base_type_with_sqlstate_0a000() {
    // Arrange
    let fixture = fixture_with_state("enum_domain_base");

    // Act
    let error = fixture.error("CREATE DOMAIN checked_state AS state_code");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

#[test]
fn should_reject_altering_a_column_to_an_enum_type_with_sqlstate_0a000() {
    // Arrange
    let fixture = fixture_with_state("enum_alter_column_type");
    fixture
        .execute("CREATE TABLE tickets (id int PRIMARY KEY, s text)")
        .unwrap();

    // Act
    let scalar_target =
        fixture.error("ALTER TABLE tickets ALTER COLUMN s TYPE state_code USING s::state_code");
    let array_target =
        fixture.error("ALTER TABLE tickets ALTER COLUMN s TYPE state_code[] USING NULL");

    // Assert
    assert_eq!(scalar_target.sql_state(), "0A000");
    assert_eq!(array_target.sql_state(), "0A000");
}

#[test]
fn should_accept_not_null_and_default_on_an_enum_column() {
    // Arrange
    let fixture = fixture_with_state("enum_not_null_default");
    fixture
        .execute("CREATE TABLE jobs (id int PRIMARY KEY, st state_code NOT NULL DEFAULT 'closed')")
        .unwrap();

    // Act
    fixture.execute("INSERT INTO jobs (id) VALUES (1)").unwrap();
    let null_error = fixture.error("INSERT INTO jobs VALUES (2, NULL)");
    let stored = scalar(&fixture, "SELECT st::text FROM jobs WHERE id = 1");

    // Assert
    assert_eq!(stored.as_deref(), Some("closed"));
    assert_eq!(null_error.sql_state(), "23502");
}

#[test]
fn should_reject_dropping_a_schema_that_still_holds_an_enum() {
    // Arrange
    let fixture = sql_fixture("enum_drop_schema_restrict", &["CREATE SCHEMA app"]);
    fixture
        .execute("CREATE TYPE app.state_code AS ENUM ('open')")
        .unwrap();

    // Act
    let restricted = fixture.execute("DROP SCHEMA app");
    fixture.execute("DROP TYPE app.state_code").unwrap();
    let emptied = fixture.execute("DROP SCHEMA app");

    // Assert
    assert!(restricted.is_err(), "a schema with an enum is not empty");
    assert!(
        emptied.is_ok(),
        "the schema is empty after the type is dropped"
    );
}

#[test]
fn should_drop_schema_enums_with_cascade_when_nothing_outside_uses_them() {
    // Arrange
    let fixture = sql_fixture("enum_cascade_inside_only", &["CREATE SCHEMA s1"]);
    fixture
        .execute("CREATE TYPE s1.state_code AS ENUM ('open')")
        .unwrap();
    fixture
        .execute("CREATE TABLE s1.jobs (id int, st s1.state_code)")
        .unwrap();

    // Act
    let dropped = fixture.execute("DROP SCHEMA s1 CASCADE");
    let gone = fixture.execute("SELECT 'open'::s1.state_code::text");

    // Assert
    assert!(dropped.is_ok(), "all dependents are inside the schema");
    assert!(gone.is_err());
}

#[test]
fn should_reject_schema_cascade_when_a_table_outside_the_schema_uses_the_enum() {
    // Arrange
    let fixture = sql_fixture(
        "enum_cascade_outside_dependent",
        &["CREATE SCHEMA s1", "CREATE SCHEMA s2"],
    );
    fixture
        .execute("CREATE TYPE s1.state_code AS ENUM ('open')")
        .unwrap();
    fixture
        .execute("CREATE TABLE s2.jobs (id int, st s1.state_code)")
        .unwrap();

    // Act
    let error = fixture.error("DROP SCHEMA s1 CASCADE");
    let type_survives = scalar(&fixture, "SELECT 'open'::s1.state_code::text");
    let table_survives = fixture.execute("SELECT st::text FROM s2.jobs");

    // Assert
    assert_eq!(error.sql_state(), "2BP01");
    assert_eq!(type_survives.as_deref(), Some("open"));
    assert!(table_survives.is_ok(), "the table must survive");
}
