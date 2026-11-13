//! `pg_identify_object` over the object classes Cassie models (#817, #818).
//! Expected rows come from a PostgreSQL 18.6 oracle.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

const SETUP: &[&str] = &[
    "CREATE SCHEMA ident",
    "CREATE TYPE ident.pair AS (left_side int, right_side text)",
    "CREATE TABLE ident.events (id INT PRIMARY KEY, label TEXT)",
    "CREATE INDEX events_label_idx ON ident.events (label)",
    "CREATE VIEW ident.event_labels AS SELECT label FROM ident.events",
    "CREATE SEQUENCE ident.event_ids",
    "CREATE FUNCTION ident.double_it(x INT) RETURNS INT AS \"x * 2\"",
    "CREATE PROCEDURE ident.touch(note TEXT) AS \"SELECT 1\"",
];

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(label, SETUP)
}

fn text(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |text| Value::String(text.to_string()))
}

fn identity_row(values: [Option<&str>; 4]) -> Vec<Value> {
    values.map(text).to_vec()
}

fn identified(fixture: &SqlFixture, call: &str) -> Vec<Vec<Value>> {
    fixture.rows(&format!("SELECT * FROM pg_identify_object({call})"))
}

#[test]
fn should_identify_a_table() {
    // Arrange
    let fixture = fixture("identify_table");

    // Act
    let rows = identified(
        &fixture,
        "'pg_class'::regclass, 'ident.events'::regclass, 0",
    );

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("table"),
            Some("ident"),
            Some("events"),
            Some("ident.events"),
        ])]
    );
}

#[test]
fn should_identify_a_view() {
    // Arrange
    let fixture = fixture("identify_view");

    // Act
    let rows = identified(
        &fixture,
        "'pg_class'::regclass, 'ident.event_labels'::regclass, 0",
    );

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("view"),
            Some("ident"),
            Some("event_labels"),
            Some("ident.event_labels"),
        ])]
    );
}

#[test]
fn should_identify_a_sequence() {
    // Arrange
    let fixture = fixture("identify_sequence");

    // Act
    let rows = identified(
        &fixture,
        "'pg_class'::regclass, 'ident.event_ids'::regclass, 0",
    );

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("sequence"),
            Some("ident"),
            Some("event_ids"),
            Some("ident.event_ids"),
        ])]
    );
}

#[test]
fn should_identify_an_index() {
    // Arrange
    let fixture = fixture("identify_index");

    // Act
    let rows = identified(
        &fixture,
        "'pg_class'::regclass, 'ident.events_label_idx'::regclass, 0",
    );

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("index"),
            Some("ident"),
            Some("events_label_idx"),
            Some("ident.events_label_idx"),
        ])]
    );
}

#[test]
fn should_identify_the_relation_of_a_composite_type() {
    // Arrange
    let fixture = fixture("identify_composite_relation");

    // Act
    let rows = identified(&fixture, "'pg_class'::regclass, 'ident.pair'::regclass, 0");

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("composite type"),
            Some("ident"),
            Some("pair"),
            Some("ident.pair"),
        ])]
    );
}

#[test]
fn should_identify_a_table_column() {
    // Arrange
    let fixture = fixture("identify_table_column");

    // Act
    let rows = identified(
        &fixture,
        "'pg_class'::regclass, 'ident.events'::regclass, 2",
    );

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("table column"),
            Some("ident"),
            Some("events"),
            Some("ident.events.label"),
        ])]
    );
}

#[test]
fn should_identify_a_view_column() {
    // Arrange
    let fixture = fixture("identify_view_column");

    // Act
    let rows = identified(
        &fixture,
        "'pg_class'::regclass, 'ident.event_labels'::regclass, 1",
    );

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("view column"),
            Some("ident"),
            Some("event_labels"),
            Some("ident.event_labels.label"),
        ])]
    );
}

#[test]
fn should_identify_a_composite_type_column() {
    // Arrange
    let fixture = fixture("identify_composite_column");

    // Act
    let rows = identified(&fixture, "'pg_class'::regclass, 'ident.pair'::regclass, 2");

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("composite type column"),
            Some("ident"),
            Some("pair"),
            Some("ident.pair.right_side"),
        ])]
    );
}

#[test]
fn should_return_only_the_column_type_for_a_missing_column() {
    // Arrange
    let fixture = fixture("identify_missing_column");

    // Act
    let rows = identified(
        &fixture,
        "'pg_class'::regclass, 'ident.events'::regclass, 9",
    );

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([Some("table column"), None, None, None])]
    );
}

#[test]
fn should_identify_a_builtin_type_by_its_sql_spelling() {
    // Arrange
    let fixture = fixture("identify_builtin_type");

    // Act
    let rows = identified(&fixture, "'pg_type'::regclass, 'int4'::regtype, 0");

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("type"),
            Some("pg_catalog"),
            Some("int4"),
            Some("integer"),
        ])]
    );
}

#[test]
fn should_qualify_a_builtin_type_without_an_sql_spelling() {
    // Arrange
    let fixture = fixture("identify_qualified_builtin_type");

    // Act
    let rows = identified(&fixture, "'pg_type'::regclass, 'text'::regtype, 0");

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("type"),
            Some("pg_catalog"),
            Some("text"),
            Some("pg_catalog.text"),
        ])]
    );
}

#[test]
fn should_quote_a_keyword_type_name() {
    // Arrange
    let fixture = fixture("identify_keyword_type");

    // Act
    let rows = identified(&fixture, "'pg_type'::regclass, 'varchar'::regtype, 0");

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("type"),
            Some("pg_catalog"),
            Some("\"varchar\""),
            Some("character varying"),
        ])]
    );
}

#[test]
fn should_identify_a_builtin_array_type() {
    // Arrange
    let fixture = fixture("identify_builtin_array_type");

    // Act
    let rows = identified(&fixture, "'pg_type'::regclass, 'text[]'::regtype, 0");

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("type"),
            Some("pg_catalog"),
            Some("_text"),
            Some("pg_catalog.text[]"),
        ])]
    );
}

#[test]
fn should_identify_a_composite_type() {
    // Arrange
    let fixture = fixture("identify_composite_type");

    // Act
    let rows = identified(&fixture, "'pg_type'::regclass, 'ident.pair'::regtype, 0");

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("type"),
            Some("ident"),
            Some("pair"),
            Some("ident.pair"),
        ])]
    );
}

#[test]
fn should_identify_a_composite_array_type() {
    // Arrange
    let fixture = fixture("identify_composite_array_type");

    // Act
    let rows = identified(&fixture, "'pg_type'::regclass, 'ident.pair[]'::regtype, 0");

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("type"),
            Some("ident"),
            Some("_pair"),
            Some("ident.pair[]"),
        ])]
    );
}

#[test]
fn should_identify_a_schema() {
    // Arrange
    let fixture = fixture("identify_schema");

    // Act
    let rows = identified(
        &fixture,
        "'pg_namespace'::regclass, 'ident'::regnamespace, 0",
    );

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("schema"),
            None,
            Some("ident"),
            Some("ident"),
        ])]
    );
}

#[test]
fn should_identify_a_user_function_by_its_signature() {
    // Arrange
    let fixture = fixture("identify_user_function");

    // Act
    let rows = identified(
        &fixture,
        "'pg_proc'::regclass, 'ident.double_it(integer)'::regprocedure, 0",
    );

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("function"),
            Some("ident"),
            None,
            Some("ident.double_it(integer)"),
        ])]
    );
}

#[test]
fn should_identify_a_user_procedure_by_its_signature() {
    // Arrange
    let fixture = fixture("identify_user_procedure");

    // Act
    let rows = identified(
        &fixture,
        "'pg_proc'::regclass, 'ident.touch(text)'::regprocedure, 0",
    );

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("procedure"),
            Some("ident"),
            None,
            Some("ident.touch(text)"),
        ])]
    );
}

#[test]
fn should_identify_a_builtin_function_with_qualified_argument_types() {
    // Arrange
    let fixture = fixture("identify_builtin_function");

    // Act
    let rows = identified(
        &fixture,
        "'pg_proc'::regclass, 'abs(integer)'::regprocedure, 0",
    );

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([
            Some("function"),
            Some("pg_catalog"),
            None,
            Some("pg_catalog.abs(integer)"),
        ])]
    );
}

#[test]
fn should_return_only_the_class_description_for_a_missing_relation() {
    // Arrange
    let fixture = fixture("identify_missing_relation");

    // Act
    let rows = identified(&fixture, "'pg_class'::regclass, 4000000000, 0");

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([Some("relation"), None, None, None])]
    );
}

#[test]
fn should_return_only_the_class_description_for_a_missing_type() {
    // Arrange
    let fixture = fixture("identify_missing_type");

    // Act
    let rows = identified(&fixture, "'pg_type'::regclass, 4000000000, 0");

    // Assert
    assert_eq!(rows, vec![identity_row([Some("type"), None, None, None])]);
}

#[test]
fn should_return_only_the_class_description_for_a_missing_routine() {
    // Arrange
    let fixture = fixture("identify_missing_routine");

    // Act
    let rows = identified(&fixture, "'pg_proc'::regclass, 4000000000, 0");

    // Assert
    assert_eq!(
        rows,
        vec![identity_row([Some("routine"), None, None, None])]
    );
}

#[test]
fn should_return_only_the_class_description_for_a_missing_schema() {
    // Arrange
    let fixture = fixture("identify_missing_schema");

    // Act
    let rows = identified(&fixture, "'pg_namespace'::regclass, 4000000000, 0");

    // Assert
    assert_eq!(rows, vec![identity_row([Some("schema"), None, None, None])]);
}

#[test]
fn should_return_an_all_null_row_for_a_null_class() {
    // Arrange
    let fixture = fixture("identify_null_class");

    // Act
    let rows = identified(&fixture, "NULL, 1, 0");

    // Assert
    assert_eq!(rows, vec![identity_row([None, None, None, None])]);
}

#[test]
fn should_reject_an_object_class_cassie_does_not_model() {
    // Arrange
    let fixture = fixture("identify_unsupported_class");

    // Act
    let error = fixture.error("SELECT * FROM pg_identify_object(999999, 1, 0)");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

#[test]
fn should_identify_each_relation_through_a_lateral_catalog_join() {
    // Arrange
    let fixture = fixture("identify_lateral_join");

    // Act
    let rows = fixture.rows(
        "SELECT identified.identity \
         FROM pg_class AS relation, \
              pg_identify_object('pg_class'::regclass, relation.oid, 0) AS identified \
         WHERE relation.relname = 'events'",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::String("ident.events".to_string())]]);
}
