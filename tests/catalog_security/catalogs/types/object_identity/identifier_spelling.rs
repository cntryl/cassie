//! Quoting, case and malformed-name rules of `reg*` input and output (#818).
//! Expected values come from a PostgreSQL 18.6 oracle.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

const SETUP: &[&str] = &[
    "CREATE SCHEMA \"q\"\"s\"",
    "CREATE SCHEMA s1",
    "CREATE TYPE pair AS (a int, b text)",
    "CREATE TABLE items (id INT PRIMARY KEY)",
];

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(label, SETUP)
}

fn sql_state(fixture: &SqlFixture, sql: &str) -> String {
    fixture.error(sql).sql_state().to_string()
}

#[test]
fn should_render_a_schema_name_holding_a_quote_with_one_level_of_escaping() {
    // Arrange
    let fixture = fixture("spelling_regnamespace_quote");

    // Act
    let rows = fixture.rows("SELECT '\"q\"\"s\"'::regnamespace::text");

    // Assert
    assert_eq!(rows, vec![vec![Value::String("\"q\"\"s\"".to_string())]]);
}

#[test]
fn should_round_trip_a_schema_name_holding_a_quote() {
    // Arrange
    let fixture = fixture("spelling_regnamespace_round_trip");

    // Act
    let rows = fixture
        .rows("SELECT '\"q\"\"s\"'::regnamespace::text::regnamespace = '\"q\"\"s\"'::regnamespace");

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_match_a_quoted_type_name_exactly() {
    // Arrange
    let fixture = fixture("spelling_quoted_type");

    // Act
    let rows = fixture.rows(
        "SELECT to_regtype('\"PAIR\"') IS NULL, to_regtype('\"pair\"') IS NOT NULL, \
                to_regtype('PAIR') IS NOT NULL",
    );

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true)
        ]]
    );
}

#[test]
fn should_match_a_quoted_schema_name_exactly() {
    // Arrange
    let fixture = fixture("spelling_quoted_schema");

    // Act
    let rows = fixture.rows(
        "SELECT to_regnamespace('\"S1\"') IS NULL, to_regnamespace('\"s1\"') IS NOT NULL, \
                to_regnamespace('S1') IS NOT NULL",
    );

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true)
        ]]
    );
}

#[test]
fn should_match_a_quoted_relation_name_exactly() {
    // Arrange
    let fixture = fixture("spelling_quoted_relation");

    // Act
    let rows = fixture.rows(
        "SELECT to_regclass('\"ITEMS\"') IS NULL, to_regclass('\"items\"') IS NOT NULL, \
                to_regclass('ITEMS') IS NOT NULL, to_regclass('\"PUBLIC\".items') IS NULL",
    );

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true)
        ]]
    );
}

#[test]
fn should_reject_a_relation_name_with_too_many_dotted_parts() {
    // Arrange
    let fixture = fixture("spelling_too_many_parts_relation");

    // Act
    let states = [
        sql_state(&fixture, "SELECT to_regclass('a.b.c.d')"),
        sql_state(&fixture, "SELECT 'a.b.c.d'::regclass"),
    ];

    // Assert
    assert_eq!(states, ["42601", "42601"]);
}

#[test]
fn should_reject_a_type_name_with_too_many_dotted_parts() {
    // Arrange
    let fixture = fixture("spelling_too_many_parts_type");

    // Act
    let state = sql_state(&fixture, "SELECT to_regtype('a.b.c.d')");

    // Assert
    assert_eq!(state, "42601");
}

#[test]
fn should_reject_a_routine_name_with_too_many_dotted_parts() {
    // Arrange
    let fixture = fixture("spelling_too_many_parts_routine");

    // Act
    let states = [
        sql_state(&fixture, "SELECT to_regproc('a.b.c.d')"),
        sql_state(&fixture, "SELECT to_regprocedure('a.b.c.d(int)')"),
    ];

    // Assert
    assert_eq!(states, ["42601", "42601"]);
}

#[test]
fn should_reject_a_relation_name_qualified_with_another_database() {
    // Arrange
    let fixture = fixture("spelling_cross_database");

    // Act
    let states = [
        sql_state(&fixture, "SELECT to_regclass('otherdb.public.items')"),
        sql_state(&fixture, "SELECT 'otherdb.public.items'::regclass"),
    ];

    // Assert
    assert_eq!(states, ["0A000", "0A000"]);
}

#[test]
fn should_return_null_for_a_two_part_schema_name() {
    // Arrange
    let fixture = fixture("spelling_two_part_schema");

    // Act
    let rows = fixture.rows("SELECT to_regnamespace('a.b') IS NULL");

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}
