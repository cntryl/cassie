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
    assert_failure(sql, "2203F", "must return single scalar item");
}

#[test]
fn should_raise_the_cardinality_error_given_several_items_and_error_on_error() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":[1,2]}'::jsonb, '$' \
               COLUMNS (a int PATH '$.a[*]' ERROR ON ERROR))";
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
    assert_failure(sql, "2203A", "does not contain key");
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

#[test]
fn should_not_wrap_a_single_item_given_a_conditional_wrapper() {
    // Arrange
    let sql =
        "SELECT * FROM JSON_TABLE('{\"a\":1,\"o\":{\"k\":1},\"s\":\"x\"}'::jsonb, '$' COLUMNS (\
               a jsonb PATH '$.a' WITH CONDITIONAL WRAPPER, \
               o jsonb PATH '$.o' WITH CONDITIONAL WRAPPER, \
               s jsonb PATH '$.s' WITH CONDITIONAL WRAPPER))";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1"), Some("{\"k\": 1}"), Some("\"x\"")]]);
}

#[test]
fn should_wrap_every_single_item_given_an_unconditional_wrapper() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":1,\"o\":{\"k\":1}}'::jsonb, '$' COLUMNS (\
               a jsonb PATH '$.a' WITH UNCONDITIONAL WRAPPER, \
               o jsonb PATH '$.o' WITH UNCONDITIONAL WRAPPER))";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("[1]"), Some("[{\"k\": 1}]")]]);
}

#[test]
fn should_raise_a_missing_key_error_given_an_exists_column_with_error_on_error() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":1}'::jsonb, '$' \
               COLUMNS (c int EXISTS PATH 'strict $.q' ERROR ON ERROR))";
    // Act
    // Assert
    assert_failure(sql, "2203A", "does not contain key");
}

#[test]
fn should_return_null_given_a_value_longer_than_varchar_without_on_error() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":\"str\"}'::jsonb, '$' \
               COLUMNS (b varchar(2) PATH '$.a'))";
    // Act
    // Assert
    assert_rows(sql, &[&[None]]);
}

#[test]
fn should_raise_value_too_long_given_varchar_and_error_on_error() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":\"str\"}'::jsonb, '$' \
               COLUMNS (b varchar(2) PATH '$.a' ERROR ON ERROR))";
    // Act
    // Assert
    assert_failure(sql, "22001", "value too long");
}

#[test]
fn should_return_empty_containers_given_empty_behaviors_on_query_columns() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":1}'::jsonb, '$' COLUMNS (\
               x jsonb PATH '$.zz' EMPTY ARRAY ON EMPTY, \
               y jsonb PATH '$.zz' EMPTY OBJECT ON EMPTY))";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("[]"), Some("{}")]]);
}

#[test]
fn should_reject_empty_array_on_a_scalar_column_naming_the_error_clause() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":1}'::jsonb, '$' \
               COLUMNS (e text PATH '$.a' EMPTY ARRAY ON ERROR))";
    // Act
    // Assert
    assert_failure(sql, "42601", "invalid ON ERROR behavior");
}

#[test]
fn should_accept_a_function_call_default() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"a\":1}'::jsonb, '$' \
               COLUMNS (v int PATH '$.zz' DEFAULT abs(-1) ON EMPTY))";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1")]]);
}

#[test]
fn should_accept_a_column_named_nested() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('{\"nested\":4}'::jsonb, '$' \
               COLUMNS (nested int PATH '$.nested'))";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("4")]]);
}

#[test]
fn should_accept_a_qualified_context_column_named_columns() {
    // Arrange
    let sql = "SELECT j.v FROM (SELECT '[7]'::jsonb AS columns) AS t, \
               JSON_TABLE(t.columns, '$[*]' COLUMNS (v int PATH '$')) AS j";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("7")]]);
}

#[test]
fn should_return_null_padding_given_a_failing_nested_path_without_error_on_error() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('[{\"a\":[1]}]'::jsonb, '$[*]' COLUMNS (\
               p int PATH '$.a[0]', NESTED PATH 'strict $.q' COLUMNS (x int PATH '$')))";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1"), None]]);
}

#[test]
fn should_raise_a_missing_key_error_given_a_failing_nested_path_and_error_on_error() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('[{\"a\":[1]}]'::jsonb, '$[*]' COLUMNS (\
               p int PATH '$.a[0]', NESTED PATH 'strict $.q' COLUMNS (x int PATH '$')) \
               ERROR ON ERROR)";
    // Act
    // Assert
    assert_failure(sql, "2203A", "does not contain key");
}
