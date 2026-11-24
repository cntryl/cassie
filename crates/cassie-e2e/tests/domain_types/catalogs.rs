//! `pg_type`, `pg_attribute` and `regtype` publication for domain types.

use crate::support_composite_types::only_text;
use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE DOMAIN posint AS int NOT NULL CHECK (VALUE > 0)",
            "CREATE TABLE items (id int, qty posint)",
        ],
    )
}

#[test]
fn should_publish_a_typtype_d_row_for_the_domain() {
    // Arrange
    let fixture = fixture("domain_pg_type_row");

    // Act
    let rows = fixture.rows(
        "SELECT typtype, typnotnull, typbasetype = 'int4'::regtype::oid, typtypmod \
         FROM pg_type WHERE typname = 'posint'",
    );

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::String("d".into()),
            Value::Bool(true),
            Value::Bool(true),
            Value::Int64(-1),
        ]]
    );
}

#[test]
fn should_publish_a_typmod_for_a_sized_base_type() {
    // Arrange
    let fixture = sql_fixture(
        "domain_pg_type_typmod",
        &["CREATE DOMAIN code AS varchar(8)"],
    );

    // Act
    let rows = fixture.rows("SELECT typtypmod FROM pg_type WHERE typname = 'code'");

    // Assert
    assert_eq!(
        rows,
        vec![vec![Value::Int64(12)]],
        "varchar(8) is typmod 8 + 4"
    );
}

#[test]
fn should_publish_an_array_type_row_for_the_domain() {
    // Arrange
    let fixture = fixture("domain_pg_type_array");

    // Act
    let linked = fixture.rows(
        "SELECT count(*) FROM pg_type d JOIN pg_type a ON a.oid = d.typarray \
         WHERE d.typname = 'posint' AND a.typname = '_posint' AND a.typelem = d.oid",
    );

    // Assert
    assert_eq!(linked, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_report_the_domain_as_the_attribute_type_of_a_domain_column() {
    // Arrange
    let fixture = fixture("domain_pg_attribute");

    // Act
    let rows = fixture.rows(
        "SELECT count(*) FROM pg_attribute a JOIN pg_type t ON t.oid = a.atttypid \
         WHERE a.attname = 'qty' AND t.typname = 'posint'",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_resolve_a_domain_name_with_regtype_in_both_directions() {
    // Arrange
    let fixture = fixture("domain_regtype");

    // Act
    let label = only_text(&fixture.rows("SELECT 'posint'::regtype::text"));
    let round_trip =
        fixture.rows("SELECT 'posint'::regtype::oid = oid FROM pg_type WHERE typname = 'posint'");

    // Assert
    assert_eq!(label, Some("posint".to_owned()));
    assert_eq!(round_trip, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_remove_the_pg_type_rows_of_a_dropped_domain() {
    // Arrange
    let fixture = sql_fixture("domain_pg_type_drop", &["CREATE DOMAIN posint AS int"]);

    // Act
    fixture.execute("DROP DOMAIN posint").unwrap();
    let rows = fixture.rows("SELECT count(*) FROM pg_type WHERE typname IN ('posint', '_posint')");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(0)]]);
}

#[test]
fn should_report_typmod_minus_one_for_a_domain_column_over_varchar() {
    // Arrange
    let fixture = sql_fixture(
        "domain_attribute_typmod",
        &[
            "CREATE DOMAIN code AS varchar(8)",
            "CREATE TABLE t (c code)",
        ],
    );

    // Act
    let rows = fixture.rows("SELECT atttypmod FROM pg_attribute WHERE attname = 'c'");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(-1)]]);
}

// Authored, not run: regtype visibility follows the first type-namespace match (review nit).

#[test]
fn should_qualify_a_domain_label_when_an_earlier_schema_holds_a_table_row_type_of_that_name() {
    // Arrange: a database-scoped session, so tables carry their schema.
    let fixture = sql_fixture("domain_regtype_shadowed_by_row_type", &[]);
    fixture.execute_as_root("CREATE DATABASE scratch").unwrap();
    let session = fixture
        .cassie
        .create_session("tester", Some("scratch".to_owned()));
    for sql in [
        "CREATE SCHEMA a",
        "CREATE TABLE a.posint (x int)",
        "CREATE DOMAIN public.posint AS int",
        "SET search_path TO a, public",
    ] {
        fixture.cassie.execute_sql(&session, sql, vec![]).unwrap();
    }

    // Act
    let rows = fixture
        .cassie
        .execute_sql(
            &session,
            "SELECT oid::regtype::text FROM pg_type WHERE typname = 'posint' AND typtype = 'd'",
            vec![],
        )
        .unwrap();
    let label = only_text(rows.rows());

    // Assert
    assert_eq!(label.as_deref(), Some("public.posint"));
}
