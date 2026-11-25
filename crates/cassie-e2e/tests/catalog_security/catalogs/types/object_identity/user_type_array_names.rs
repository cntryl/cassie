//! `regtype` input accepts every PostgreSQL spelling of a user type's array
//! type (#818, #827, #828). Expected values come from a PostgreSQL 18.6 oracle.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

const SETUP: &[&str] = &[
    "CREATE TYPE pair AS (a int, b text)",
    "CREATE DOMAIN pos AS int",
    "CREATE TABLE rowt (id INT PRIMARY KEY)",
    "CREATE SCHEMA s2",
    "CREATE TYPE s2.lvl AS ENUM ('lo', 'hi')",
];

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(label, SETUP)
}

fn label(fixture: &SqlFixture, spelling: &str) -> Value {
    fixture
        .rows(&format!("SELECT '{spelling}'::regtype::text"))
        .remove(0)
        .remove(0)
}

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

#[test]
fn should_resolve_a_composite_array_type_by_its_underscore_name() {
    // Arrange
    let fixture = fixture("array_name_composite_underscore");

    // Act
    let labels = ["_pair", "public._pair"].map(|spelling| label(&fixture, spelling));

    // Assert
    assert_eq!(labels, [text("pair[]"), text("pair[]")]);
}

#[test]
fn should_collapse_repeated_dimensions_of_a_composite_array() {
    // Arrange
    let fixture = fixture("array_name_composite_dimensions");

    // Act
    let label = label(&fixture, "pair[][]");

    // Assert
    assert_eq!(label, text("pair[]"));
}

#[test]
fn should_ignore_the_bound_of_a_composite_array_dimension() {
    // Arrange
    let fixture = fixture("array_name_composite_bound");

    // Act
    let label = label(&fixture, "pair[3]");

    // Assert
    assert_eq!(label, text("pair[]"));
}

#[test]
fn should_resolve_an_enum_array_type_by_its_underscore_name() {
    // Arrange
    let fixture = fixture("array_name_enum_underscore");

    // Act
    let label = label(&fixture, "s2._lvl");

    // Assert
    assert_eq!(label, text("s2.lvl[]"));
}

#[test]
fn should_resolve_a_domain_array_type_by_its_underscore_name() {
    // Arrange
    let fixture = fixture("array_name_domain_underscore");

    // Act
    let label = label(&fixture, "_pos");

    // Assert
    assert_eq!(label, text("pos[]"));
}

#[test]
fn should_resolve_a_row_type_array_type_by_its_underscore_name() {
    // Arrange
    let fixture = fixture("array_name_row_type_underscore");

    // Act
    let label = label(&fixture, "_rowt");

    // Assert
    assert_eq!(label, text("rowt[]"));
}

#[test]
fn should_match_the_underscore_name_pg_type_publishes() {
    // Arrange
    let fixture = fixture("array_name_matches_pg_type");

    // Act
    let rows = fixture.rows(
        "SELECT count(*) FROM pg_type \
         WHERE typname = '_pair' AND oid = '_pair'::regtype",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}
