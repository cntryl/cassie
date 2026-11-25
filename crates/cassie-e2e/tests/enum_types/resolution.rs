//! `ALTER TYPE` and `DROP TYPE` resolve an unqualified name through the search
//! path across type kinds: the first schema declaring the name decides (#827).

use crate::support_enum_types::scalar;
use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn fixture(label: &str, search_path: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE SCHEMA z1",
            "CREATE SCHEMA z2",
            "CREATE TYPE z1.dual AS ENUM ('e1')",
            "CREATE TYPE z2.dual AS (a int)",
            search_path,
        ],
    )
}

fn type_count(fixture: &SqlFixture, schema: &str) -> Option<String> {
    scalar(
        fixture,
        &format!(
            "SELECT count(*)::text FROM pg_catalog.pg_type t \
             JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace \
             WHERE t.typname = 'dual' AND n.nspname = '{schema}'"
        ),
    )
}

#[test]
fn should_alter_the_enum_that_precedes_a_composite_in_the_search_path() {
    // Arrange
    let fixture = fixture("enum_resolution_alter", "SET search_path = z1, z2, public");

    // Act
    let altered = fixture.execute("ALTER TYPE dual ADD VALUE 'e2'");
    let labels = scalar(&fixture, "SELECT enum_range(NULL::z1.dual)::text");

    // Assert
    assert!(altered.is_ok());
    assert_eq!(labels.as_deref(), Some("{e1,e2}"));
}

#[test]
fn should_drop_the_enum_that_precedes_a_composite_in_the_search_path() {
    // Arrange
    let fixture = fixture("enum_resolution_drop", "SET search_path = z1, z2, public");

    // Act
    let dropped = fixture.execute("DROP TYPE dual");
    let enum_left = type_count(&fixture, "z1");
    let composite_left = type_count(&fixture, "z2");

    // Assert
    assert!(dropped.is_ok());
    assert_eq!(enum_left.as_deref(), Some("0"));
    assert_eq!(composite_left.as_deref(), Some("1"));
}

#[test]
fn should_not_treat_a_name_as_an_enum_when_a_composite_precedes_it() {
    // Arrange
    let fixture = fixture(
        "enum_resolution_shadowed",
        "SET search_path = z2, z1, public",
    );

    // Act
    let error = fixture.error("ALTER TYPE dual ADD VALUE 'e2'");
    let labels = scalar(&fixture, "SELECT enum_range(NULL::z1.dual)::text");

    // Assert
    assert_eq!(error.sql_state(), "42809");
    assert_eq!(labels.as_deref(), Some("{e1}"));
}
