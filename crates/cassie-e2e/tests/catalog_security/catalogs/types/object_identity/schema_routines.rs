//! A schema's functions and procedures are schema objects: they block a plain
//! `DROP SCHEMA` and a cascade drops them (#818). Expected behavior comes from
//! a PostgreSQL 18.6 oracle.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

const SETUP: &[&str] = &[
    "CREATE SCHEMA dsc",
    "CREATE FUNCTION dsc.g(a INT) RETURNS INT AS \"a * 7\"",
    "CREATE SCHEMA psc",
    "CREATE PROCEDURE psc.touch(note TEXT) AS \"SELECT 1\"",
];

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(label, SETUP)
}

fn truth(fixture: &SqlFixture, sql: &str) -> Vec<Vec<Value>> {
    fixture.rows(sql)
}

#[test]
fn should_refuse_to_drop_a_schema_that_holds_a_function() {
    // Arrange
    let fixture = fixture("schema_routines_restrict_function");

    // Act
    let dropped = fixture.execute("DROP SCHEMA dsc");

    // Assert
    assert!(dropped.is_err());
    assert_eq!(
        truth(
            &fixture,
            "SELECT to_regnamespace('dsc') IS NOT NULL, \
                    to_regprocedure('dsc.g(int)') IS NOT NULL"
        ),
        vec![vec![Value::Bool(true), Value::Bool(true)]]
    );
}

#[test]
fn should_refuse_to_drop_a_schema_that_holds_a_procedure() {
    // Arrange
    let fixture = fixture("schema_routines_restrict_procedure");

    // Act
    let dropped = fixture.execute("DROP SCHEMA psc");

    // Assert
    assert!(dropped.is_err());
    assert_eq!(
        truth(&fixture, "SELECT to_regnamespace('psc') IS NOT NULL"),
        vec![vec![Value::Bool(true)]]
    );
}

#[test]
fn should_drop_the_functions_of_a_schema_dropped_with_cascade() {
    // Arrange
    let fixture = fixture("schema_routines_cascade_function");

    // Act
    fixture
        .execute("DROP SCHEMA dsc CASCADE")
        .expect("drop the schema and its function");
    fixture
        .execute("CREATE SCHEMA dsc")
        .expect("recreate the schema");

    // Assert
    assert_eq!(
        truth(&fixture, "SELECT to_regprocedure('dsc.g(int)') IS NULL"),
        vec![vec![Value::Bool(true)]]
    );
    assert!(fixture.execute("SELECT dsc.g(2)").is_err());
}

#[test]
fn should_free_a_function_name_when_its_schema_is_dropped_with_cascade() {
    // Arrange
    let fixture = fixture("schema_routines_cascade_recreate");
    fixture
        .execute("DROP SCHEMA dsc CASCADE")
        .expect("drop the schema and its function");
    fixture
        .execute("CREATE SCHEMA dsc")
        .expect("recreate the schema");

    // Act
    let recreated = fixture.execute("CREATE FUNCTION dsc.g(a INT) RETURNS INT AS \"a\"");

    // Assert
    assert!(recreated.is_ok());
    assert_eq!(
        truth(&fixture, "SELECT dsc.g(2)"),
        vec![vec![Value::Int64(2)]]
    );
}

#[test]
fn should_drop_the_procedures_of_a_schema_dropped_with_cascade() {
    // Arrange
    let fixture = fixture("schema_routines_cascade_procedure");

    // Act
    fixture
        .execute("DROP SCHEMA psc CASCADE")
        .expect("drop the schema and its procedure");
    fixture
        .execute("CREATE SCHEMA psc")
        .expect("recreate the schema");

    // Assert
    assert_eq!(
        truth(
            &fixture,
            "SELECT to_regprocedure('psc.touch(text)') IS NULL"
        ),
        vec![vec![Value::Bool(true)]]
    );
}
