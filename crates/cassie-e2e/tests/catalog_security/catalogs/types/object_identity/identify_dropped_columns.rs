//! `pg_identify_object` column numbers are `pg_attribute.attnum` values, and
//! its arguments may be scalar subqueries (#817, #818). Expected rows come
//! from a PostgreSQL 18.6 oracle.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

const SETUP: &[&str] = &[
    "CREATE TYPE dct AS (a int, b int, c int)",
    "ALTER TYPE dct DROP ATTRIBUTE a",
    "CREATE TABLE dtab (id INT PRIMARY KEY, x INT, y INT)",
    "ALTER TABLE dtab DROP COLUMN x",
];

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(label, SETUP)
}

fn identity_row(values: [Option<&str>; 4]) -> Vec<Value> {
    values
        .map(|value| value.map_or(Value::Null, |text| Value::String(text.to_string())))
        .to_vec()
}

fn identified(fixture: &SqlFixture, call: &str) -> Vec<Vec<Value>> {
    fixture.rows(&format!("SELECT * FROM pg_identify_object({call})"))
}

#[test]
fn should_identify_a_composite_attribute_by_its_attribute_number_after_a_drop() {
    // Arrange
    let fixture = fixture("identify_composite_attnum");

    // Act
    let second = identified(&fixture, "'pg_class'::regclass, 'dct'::regclass, 2");
    let third = identified(&fixture, "'pg_class'::regclass, 'dct'::regclass, 3");

    // Assert
    assert_eq!(
        second,
        vec![identity_row([
            Some("composite type column"),
            Some("public"),
            Some("dct"),
            Some("public.dct.b"),
        ])]
    );
    assert_eq!(
        third,
        vec![identity_row([
            Some("composite type column"),
            Some("public"),
            Some("dct"),
            Some("public.dct.c"),
        ])]
    );
}

#[test]
fn should_identify_a_dropped_composite_attribute_by_its_placeholder_name() {
    // Arrange
    let fixture = fixture("identify_composite_dropped");

    // Act
    let rows = identified(&fixture, "'pg_class'::regclass, 'dct'::regclass, 1");

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("composite type column"),
            Some("public"),
            Some("dct"),
            Some("public.dct.\"........pg.dropped.1........\""),
        ])]
    );
}

#[test]
fn should_identify_no_composite_attribute_past_the_last_attribute_number() {
    // Arrange
    let fixture = fixture("identify_composite_past_end");

    // Act
    let rows = identified(&fixture, "'pg_class'::regclass, 'dct'::regclass, 4");

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("composite type column"),
            None,
            None,
            None
        ])]
    );
}

#[test]
fn should_identify_a_dropped_table_column_by_its_placeholder_name() {
    // Arrange
    let fixture = fixture("identify_table_dropped");

    // Act
    let rows = identified(&fixture, "'pg_class'::regclass, 'dtab'::regclass, 2");

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("table column"),
            Some("public"),
            Some("dtab"),
            Some("public.dtab.\"........pg.dropped.2........\""),
        ])]
    );
}

#[test]
fn should_keep_the_attribute_number_of_a_column_after_a_dropped_one() {
    // Arrange
    let fixture = fixture("identify_table_after_dropped");

    // Act
    let rows = identified(&fixture, "'pg_class'::regclass, 'dtab'::regclass, 3");

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("table column"),
            Some("public"),
            Some("dtab"),
            Some("public.dtab.y"),
        ])]
    );
}

#[test]
fn should_identify_an_object_named_by_a_scalar_subquery_argument() {
    // Arrange
    let fixture = fixture("identify_subquery_argument");

    // Act
    let rows = identified(
        &fixture,
        "'pg_class'::regclass, (SELECT 'dtab'::regclass::oid), 0",
    );

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("table"),
            Some("public"),
            Some("dtab"),
            Some("public.dtab"),
        ])]
    );
}

#[test]
fn should_identify_an_object_named_by_a_catalog_subquery_argument() {
    // Arrange
    let fixture = fixture("identify_catalog_subquery_argument");

    // Act
    let rows = identified(
        &fixture,
        "1259, (SELECT oid FROM pg_class WHERE relname = 'dtab'), 0",
    );

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("table"),
            Some("public"),
            Some("dtab"),
            Some("public.dtab"),
        ])]
    );
}
