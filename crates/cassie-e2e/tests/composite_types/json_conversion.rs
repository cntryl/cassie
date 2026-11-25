//! JSON conversion of composite and anonymous record values: `to_json`,
//! `to_jsonb` and `row_to_json` render a record as an object keyed by its
//! field names, as PostgreSQL 18.6 does.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn holders(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TYPE rjt AS (a int, b text)",
            "CREATE TABLE rj (id int primary key, v rjt)",
            "INSERT INTO rj VALUES (1, ROW(2, 'x'))",
        ],
    )
}

fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}

#[test]
fn should_convert_a_composite_column_to_a_json_object() {
    // Arrange
    let fixture = holders("record_json_composite_column");

    // Act
    let rows =
        fixture.rows("SELECT to_json(v)::text, to_jsonb(v)::text, row_to_json(v)::text FROM rj");

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            text(r#"{"a":2,"b":"x"}"#),
            text(r#"{"a": 2, "b": "x"}"#),
            text(r#"{"a":2,"b":"x"}"#),
        ]]
    );
}

#[test]
fn should_name_anonymous_record_fields_f1_onwards_in_json() {
    // Arrange
    let fixture = sql_fixture("record_json_anonymous", &[]);

    // Act
    let rows = fixture.rows("SELECT row_to_json(ROW(1, 'a'))::text, to_jsonb(ROW(1, 'a'))::text");

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            text(r#"{"f1":1,"f2":"a"}"#),
            text(r#"{"f1": 1, "f2": "a"}"#)
        ]]
    );
}

#[test]
fn should_convert_a_whole_table_row_with_a_nested_composite_to_json() {
    // Arrange
    let fixture = holders("record_json_whole_row");

    // Act
    let rows = fixture.rows("SELECT row_to_json(rj)::text, to_jsonb(rj)::text FROM rj");

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            text(r#"{"id":1,"v":{"a":2,"b":"x"}}"#),
            text(r#"{"v": {"a": 2, "b": "x"}, "id": 1}"#),
        ]]
    );
}

#[test]
fn should_reject_row_to_json_of_a_scalar_with_sqlstate_42883() {
    // Arrange
    let fixture = sql_fixture("record_json_scalar_argument", &[]);

    // Act
    let error = fixture.error("SELECT row_to_json(1)");

    // Assert
    assert_eq!(error.sql_state(), "42883");
}

#[test]
fn should_put_each_field_on_its_own_line_in_pretty_row_to_json() {
    // Arrange
    let fixture = sql_fixture("record_json_pretty", &[]);

    // Act
    let rows = fixture.rows("SELECT row_to_json(ROW(1, 'a'), true)::text");

    // Assert
    assert_eq!(rows, vec![vec![text("{\"f1\":1,\n \"f2\":\"a\"}")]]);
}

#[test]
fn should_return_null_for_a_null_record_in_row_to_json() {
    // Arrange
    let fixture = sql_fixture("record_json_null_record", &["CREATE TYPE nrt AS (a int)"]);

    // Act
    let rows = fixture.rows("SELECT row_to_json(NULL::nrt)");

    // Assert
    assert_eq!(rows, vec![vec![Value::Null]]);
}

#[test]
fn should_put_each_later_record_or_array_on_its_own_line_in_json_agg() {
    // Arrange
    let fixture = sql_fixture(
        "record_json_agg_layout",
        &[
            "CREATE TYPE rja AS (a int, b text)",
            "CREATE TABLE rjg (id int primary key, v rja)",
            "INSERT INTO rjg VALUES (1, ROW(1, 'x')), (2, NULL), (3, ROW(3, 'z'))",
        ],
    );

    // Act
    let rows = fixture.rows(
        "SELECT json_agg(v ORDER BY id)::text, json_agg(ARRAY[id] ORDER BY id)::text, json_agg(id ORDER BY id)::text, jsonb_agg(v ORDER BY id)::text FROM rjg",
    );

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            text("[{\"a\":1,\"b\":\"x\"}, null, \n {\"a\":3,\"b\":\"z\"}]"),
            text("[[1], \n [2], \n [3]]"),
            text("[1, 2, 3]"),
            text(r#"[{"a": 1, "b": "x"}, null, {"a": 3, "b": "z"}]"#),
        ]]
    );
}
