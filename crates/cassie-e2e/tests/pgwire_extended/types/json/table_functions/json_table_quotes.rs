//! `JSON_TABLE` quotes handling, wrapper combinations and boolean text (#829).
//! Expected rows and SQLSTATEs come from PostgreSQL 18.6.

use super::json_records_support::{assert_failure, assert_rows};

#[test]
fn should_parse_the_string_content_given_omit_quotes_on_a_jsonb_column() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":\"1\"}'::jsonb, '$' COLUMNS \
               (c jsonb PATH '$.a' OMIT QUOTES, d jsonb PATH '$.a' KEEP QUOTES)) AS t";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1"), Some("\"1\"")]]);
}

#[test]
fn should_return_null_given_omit_quotes_content_that_is_not_json() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":\"abcdef\"}'::jsonb, '$' COLUMNS \
               (c jsonb PATH '$.a' OMIT QUOTES, d json PATH '$.a' OMIT QUOTES)) AS t";
    // Act
    // Assert
    assert_rows(sql, &[&[None, None]]);
}

#[test]
fn should_raise_given_omit_quotes_content_that_is_not_json_and_error_on_error() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":\"abcdef\"}'::jsonb, '$' COLUMNS \
               (c jsonb PATH '$.a' OMIT QUOTES ERROR ON ERROR)) AS t";
    // Act
    // Assert
    assert_failure(sql, "22P02", "invalid input syntax for type json");
}

#[test]
fn should_read_a_quoted_string_given_keep_quotes_on_an_integer_column() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":\"1\"}'::jsonb, '$' COLUMNS \
               (c int PATH '$.a' KEEP QUOTES, d int PATH '$.a' OMIT QUOTES)) AS t";
    // Act
    // Assert
    assert_rows(sql, &[&[None, Some("1")]]);
}

#[test]
fn should_raise_given_keep_quotes_on_an_integer_column_and_error_on_error() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":\"1\"}'::jsonb, '$' COLUMNS \
               (c int PATH '$.a' KEEP QUOTES ERROR ON ERROR)) AS t";
    // Act
    // Assert
    assert_failure(sql, "22P02", "");
}

#[test]
fn should_require_a_json_array_given_a_string_for_an_array_column() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":\"{1}\"}'::jsonb, '$' COLUMNS \
               (c int[] PATH '$.a', d int[] PATH '$.a' OMIT QUOTES)) AS t";
    // Act
    // Assert
    assert_rows(sql, &[&[None, Some("{1}")]]);
}

#[test]
fn should_require_a_json_object_given_a_string_for_a_composite_column() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":\"(1,x)\"}'::jsonb, '$' COLUMNS \
               (c pair PATH '$.a', d pair PATH '$.a' OMIT QUOTES)) AS t";
    // Act
    // Assert
    assert_rows(sql, &[&[None, Some("(1,x)")]]);
}

#[test]
fn should_raise_given_a_string_for_a_composite_column_and_error_on_error() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":\"(1,x)\"}'::jsonb, '$' COLUMNS \
               (c pair PATH '$.a' ERROR ON ERROR)) AS t";
    // Act
    // Assert
    assert_failure(sql, "22023", "populate_composite");
}

#[test]
fn should_render_a_json_boolean_as_sql_boolean_text_given_a_value_column() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":true,\"b\":false}'::jsonb, '$' COLUMNS \
               (c text PATH '$.a', d varchar(4) PATH '$.a', e text PATH '$.b')) AS t";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("t"), Some("t"), Some("f")]]);
}

#[test]
fn should_keep_the_json_boolean_spelling_given_a_query_column() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":true}'::jsonb, '$' COLUMNS \
               (c text PATH '$.a' KEEP QUOTES, d text FORMAT JSON PATH '$.a')) AS t";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("true"), Some("true")]]);
}

#[test]
fn should_accept_keep_quotes_given_a_wrapper() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":\"x\"}'::jsonb, '$' COLUMNS \
               (c text PATH '$.a' WITH WRAPPER KEEP QUOTES, \
                d text PATH '$.a' WITH CONDITIONAL WRAPPER KEEP QUOTES)) AS t";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("[\"x\"]"), Some("\"x\"")]]);
}

#[test]
fn should_reject_omit_quotes_given_a_conditional_wrapper() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":1}'::jsonb, '$' COLUMNS \
               (a text PATH '$.a' WITH CONDITIONAL WRAPPER OMIT QUOTES)) AS t";
    // Act
    // Assert
    assert_failure(sql, "42601", "QUOTES behavior must not be specified");
}
