//! PostgreSQL 18.6 values of `JSON_TABLE` regular, ordinality and `EXISTS` columns.

use super::json_records_support::{assert_failure, assert_rows};

#[test]
fn should_project_each_row_pattern_item_given_regular_columns() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('[1,2,3]'::jsonb, '$[*]' COLUMNS (a int PATH '$'))";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1")], &[Some("2")], &[Some("3")]]);
}

#[test]
fn should_number_items_given_a_for_ordinality_column() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('[{\"a\":1},{\"a\":2}]'::jsonb, '$[*]' \
               COLUMNS (i FOR ORDINALITY, v int PATH '$.a'))";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1"), Some("1")], &[Some("2"), Some("2")]]);
}

#[test]
fn should_return_null_given_a_missing_path_without_on_empty() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":1}'::jsonb, '$' COLUMNS (v int PATH '$.zz'))";
    // Act
    // Assert
    assert_rows(sql, &[&[None]]);
}

#[test]
fn should_return_the_default_given_an_empty_path_with_default_on_empty() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":1}'::jsonb, '$' \
               COLUMNS (a text PATH '$.zz' DEFAULT 'dflt' ON EMPTY))";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("dflt")]]);
}

#[test]
fn should_return_the_default_given_a_non_scalar_item_with_default_on_error() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('[{\"a\":1},{\"a\":[1,2]}]'::jsonb, '$[*]' \
               COLUMNS (i FOR ORDINALITY, v int PATH '$.a' DEFAULT 9 ON ERROR))";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1"), Some("1")], &[Some("2"), Some("9")]]);
}

#[test]
fn should_return_null_given_a_non_scalar_item_without_on_error() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":{\"k\":1}}'::jsonb, '$' \
               COLUMNS (v int PATH '$.a'))";
    // Act
    // Assert
    assert_rows(sql, &[&[None]]);
}

#[test]
fn should_keep_quotes_given_a_text_query_column() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":\"x\"}'::jsonb, '$' \
               COLUMNS (v text FORMAT JSON PATH '$.a', w text PATH '$.a' OMIT QUOTES, \
               u text PATH '$.a'))";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("\"x\""), Some("x"), Some("x")]]);
}

#[test]
fn should_apply_wrappers_given_array_items() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":[1,2]}'::jsonb, '$' COLUMNS (\
               v jsonb PATH '$.a[*]' WITH CONDITIONAL WRAPPER, \
               w jsonb PATH '$.a' WITH CONDITIONAL WRAPPER, \
               u jsonb PATH '$.a' WITH UNCONDITIONAL WRAPPER, \
               n jsonb PATH '$.zz' WITH UNCONDITIONAL WRAPPER))";
    // Act
    // Assert
    assert_rows(
        sql,
        &[&[Some("[1, 2]"), Some("[1, 2]"), Some("[[1, 2]]"), None]],
    );
}

#[test]
fn should_evaluate_exists_columns_given_present_and_absent_paths() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":1}'::jsonb, '$' COLUMNS (\
               a int EXISTS PATH '$.a', b int EXISTS PATH '$.q'))";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1"), Some("0")]]);
}

#[test]
fn should_filter_with_passing_variables_given_a_path_predicate() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":7}'::jsonb, '$' PASSING 5 AS lim \
               COLUMNS (a int PATH '$.a ? (@ > $lim)'))";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("7")]]);
}

#[test]
fn should_return_no_rows_given_a_null_context_item() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE(NULL::jsonb, '$' COLUMNS (a int PATH '$.a'))";
    // Act
    // Assert
    assert_rows(sql, &[]);
}

#[test]
fn should_return_no_rows_given_a_failing_root_path_with_the_default_on_error() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":1}'::jsonb, 'strict $.x.y' \
               COLUMNS (a int PATH '$.a'))";
    // Act
    // Assert
    assert_rows(sql, &[]);
}

#[test]
fn should_bind_per_row_context_items_given_a_lateral_join() {
    // Arrange
    let sql = "SELECT t.id, j.i, j.v FROM (VALUES (1, '[3,4]'::jsonb), (2, '[5]'::jsonb)) \
               AS t(id, d), JSON_TABLE(t.d, '$[*]' COLUMNS (i FOR ORDINALITY, \
               v int PATH '$')) AS j ORDER BY t.id, j.i";
    // Act
    // Assert
    assert_rows(
        sql,
        &[
            &[Some("1"), Some("1"), Some("3")],
            &[Some("1"), Some("2"), Some("4")],
            &[Some("2"), Some("1"), Some("5")],
        ],
    );
}

#[test]
fn should_raise_the_column_error_given_error_on_error() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":[1,2]}'::jsonb, '$' \
               COLUMNS (a int PATH '$.a' ERROR ON ERROR))";
    // Act
    // Assert
    assert_failure(sql, "22034", "must return single scalar item");
}

#[test]
fn should_raise_the_missing_item_error_given_error_on_empty() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":7}'::jsonb, '$' PASSING 9 AS lim \
               COLUMNS (a int PATH '$.a ? (@ > $lim)' ERROR ON EMPTY))";
    // Act
    // Assert
    assert_failure(
        sql,
        "22035",
        "no SQL/JSON item found for specified path of column \"a\"",
    );
}

#[test]
fn should_raise_the_root_error_given_error_on_error_at_the_root() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":1}'::jsonb, 'strict $.x.y' \
               COLUMNS (a int PATH '$.a') ERROR ON ERROR)";
    // Act
    // Assert
    assert_failure(sql, "22039", "x");
}

#[test]
fn should_reject_json_format_given_a_non_string_output_type() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":\"x\"}'::jsonb, '$' \
               COLUMNS (a int FORMAT JSON PATH '$.a'))";
    // Act
    // Assert
    assert_failure(
        sql,
        "0A000",
        "cannot use JSON format with non-string output types",
    );
}

#[test]
fn should_reject_quotes_given_a_wrapper() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":\"x\"}'::jsonb, '$' \
               COLUMNS (a text PATH '$.a' WITH WRAPPER OMIT QUOTES))";
    // Act
    // Assert
    assert_failure(sql, "42601", "QUOTES behavior must not be specified");
}
