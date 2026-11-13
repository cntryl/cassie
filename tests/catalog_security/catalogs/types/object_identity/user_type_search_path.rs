//! Unqualified user type names follow the search path across enum, composite,
//! domain and table row types (#818, #827, #828). Expected values come from a
//! PostgreSQL 18.6 oracle.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

/// `sh2.shd` is a domain shadowing the composite `public.shd`; `z1.dual` is an
/// enum shadowing the composite `z2.dual`; the composite `sx.tsx` shadows the
/// row type of the table `public.tsx`.
const SETUP: &[&str] = &[
    "CREATE SCHEMA sh2",
    "CREATE DOMAIN sh2.shd AS int",
    "CREATE TYPE public.shd AS (a int)",
    "CREATE SCHEMA z1",
    "CREATE SCHEMA z2",
    "CREATE TYPE z1.dual AS ENUM ('a', 'b')",
    "CREATE TYPE z2.dual AS (x int)",
    "CREATE TABLE tsx (id INT PRIMARY KEY)",
    "CREATE SCHEMA sx",
    "CREATE TYPE sx.tsx AS (a int)",
];

fn fixture(label: &str, search_path: &str) -> SqlFixture {
    let fixture = sql_fixture(label, SETUP);
    fixture
        .execute(&format!("SET search_path = {search_path}"))
        .expect("set the search path");
    fixture
}

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

#[test]
fn should_resolve_an_unqualified_regtype_to_the_domain_earlier_on_the_search_path() {
    // Arrange
    let fixture = fixture("user_type_path_domain_first", "sh2, public");

    // Act
    let rows = fixture.rows("SELECT 'shd'::regtype = 'sh2.shd'::regtype");

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_qualify_the_label_of_a_composite_shadowed_by_a_domain() {
    // Arrange
    let fixture = fixture("user_type_path_shadowed_label", "sh2, public");

    // Act
    let rows = fixture.rows("SELECT 'public.shd'::regtype::text, 'sh2.shd'::regtype::text");

    // Assert
    assert_eq!(rows, vec![vec![text("public.shd"), text("shd")]]);
}

#[test]
fn should_round_trip_the_label_of_a_shadowed_composite() {
    // Arrange
    let fixture = fixture("user_type_path_shadowed_round_trip", "sh2, public");

    // Act
    let rows = fixture.rows("SELECT 'public.shd'::regtype::text::regtype = 'public.shd'::regtype");

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_bind_a_cast_to_the_domain_earlier_on_the_search_path() {
    // Arrange
    let fixture = fixture("user_type_path_cast_domain", "sh2, public");

    // Act
    let rows = fixture.rows("SELECT 1::shd + 0");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_bind_a_cast_to_the_enum_earlier_on_the_search_path() {
    // Arrange
    let fixture = fixture("user_type_path_cast_enum", "z1, z2, public");

    // Act
    let rows = fixture.rows("SELECT enum_range(null::dual)::text");

    // Assert
    assert_eq!(rows, vec![vec![text("{a,b}")]]);
}

#[test]
fn should_bind_a_cast_to_the_composite_when_its_schema_comes_first() {
    // Arrange
    let fixture = fixture("user_type_path_cast_composite", "z2, z1, public");

    // Act
    let rows = fixture.rows("SELECT 'dual'::regtype = 'z2.dual'::regtype, (ROW(7)::dual).x");

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true), Value::Int64(7)]]);
}

#[test]
fn should_qualify_the_label_of_a_row_type_shadowed_by_a_composite() {
    // Arrange
    let fixture = fixture("user_type_path_row_type_label", "sx, public");

    // Act
    let rows = fixture.rows(
        "SELECT 'public.tsx'::regtype::text, \
                'public.tsx'::regtype::text::regtype = 'public.tsx'::regtype",
    );

    // Assert
    assert_eq!(rows, vec![vec![text("public.tsx"), Value::Bool(true)]]);
}
