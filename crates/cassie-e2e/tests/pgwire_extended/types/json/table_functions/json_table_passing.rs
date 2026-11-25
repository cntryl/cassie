//! `JSON_TABLE ... PASSING` binds numeric, JSON and untyped parameter values (#829).
//! Expected rows come from PostgreSQL 18.6.

use super::json_elements_support::{error_message, row, Rows};
use super::json_records_support::assert_rows;
use super::{fixture, wire};

#[test]
fn should_bind_a_jsonb_object_as_a_json_item_given_passing() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":10}'::jsonb, '$' \
               PASSING '{\"k\":2}'::jsonb AS x COLUMNS (a int PATH '$x.k')) AS t";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("2")]]);
}

#[test]
fn should_bind_a_jsonb_array_as_a_json_item_given_passing() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":10}'::jsonb, '$' \
               PASSING '[1,2]'::jsonb AS x COLUMNS (a int PATH '$x[1]')) AS t";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("2")]]);
}

#[test]
fn should_bind_a_json_document_as_a_json_item_given_passing() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":10}'::jsonb, '$' \
               PASSING '{\"k\":2}'::json AS x COLUMNS (a int PATH '$x.k')) AS t";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("2")]]);
}

#[test]
fn should_keep_text_a_json_string_given_passing() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":10}'::jsonb, '$' \
               PASSING '{\"k\":2}'::text AS x COLUMNS (a text PATH '$x')) AS t";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("{\"k\":2}")]]);
}

#[test]
fn should_add_a_decimal_literal_given_passing() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":10}'::jsonb, '$' \
               PASSING 1.5 AS x COLUMNS (a numeric PATH '$.a + $x')) AS t";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("11.5")]]);
}

#[test]
fn should_return_a_numeric_cast_given_passing() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":10}'::jsonb, '$' \
               PASSING 2.75::numeric AS x COLUMNS (a numeric PATH '$x')) AS t";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("2.75")]]);
}

#[test]
fn should_return_an_integer_beyond_bigint_given_passing() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":10}'::jsonb, '$' \
               PASSING 12345678901234567890 AS x COLUMNS (a numeric PATH '$x')) AS t";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("12345678901234567890")]]);
}

#[test]
fn should_infer_text_given_an_untyped_passing_parameter() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":1}'::jsonb, '$' \
               PASSING $1 AS x COLUMNS (a text PATH '$x'))";
    let body: &[u8] = b"k";
    let cycle = fixture::execute_cycle(sql, Some((0, 0, Some(body))), 0, false);
    // Act
    let frames = fixture::run_cycles("json-table-passing-parameter", &[], vec![cycle]).remove(0);
    // Assert
    assert_eq!(error_message(&frames), None);
    let expected: Rows = vec![row(&[Some("k")])];
    assert_eq!(wire::data_rows(&frames), expected);
}
