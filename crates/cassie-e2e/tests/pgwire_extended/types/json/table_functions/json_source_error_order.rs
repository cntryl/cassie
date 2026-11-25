//! Error precedence and up-front validation of JSON FROM sources (#829).
//! Expected SQLSTATEs come from PostgreSQL 18.6.

use super::json_records_support::{assert_failure, failure};

#[test]
fn should_report_the_earlier_conversion_error_given_a_later_non_object_element() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_recordset('[{\"a\":\"x\"},3]') AS t(a int)";
    // Act
    // Assert
    assert_failure(sql, "22P02", "");
}

#[test]
fn should_report_the_earlier_conversion_error_given_json_text_input() {
    // Arrange
    let sql = "SELECT * FROM json_to_recordset('[{\"a\":\"x\"},3]') AS t(a int)";
    // Act
    // Assert
    assert_failure(sql, "22P02", "");
}

#[test]
fn should_report_the_earlier_conversion_error_given_populate_recordset() {
    // Arrange
    let sql = "SELECT * FROM jsonb_populate_recordset(NULL::pair, '[{\"a\":\"x\"},3]')";
    // Act
    // Assert
    assert_failure(sql, "22P02", "");
}

#[test]
fn should_report_the_earlier_non_object_element_given_a_later_conversion_error() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_recordset('[3,{\"a\":\"x\"}]') AS t(a int)";
    // Act
    // Assert
    assert_failure(sql, "22023", "must be an array of objects");
}

#[test]
fn should_reject_an_invalid_default_given_a_null_json_table_context() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE(NULL::jsonb, '$' COLUMNS \
               (b int PATH '$.x' DEFAULT 'zz' ON EMPTY)) AS t";
    // Act
    // Assert
    assert_failure(sql, "22P02", "");
}

#[test]
fn should_reject_with_ordinality_after_json_table_as_a_syntax_error() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('[1]'::jsonb, '$[*]' COLUMNS (a int PATH '$')) \
               WITH ORDINALITY AS t";
    // Act
    let (code, _) = failure(sql);
    // Assert
    assert_eq!(code.as_deref(), Some("42601"));
}
