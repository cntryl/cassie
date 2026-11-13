//! PostgreSQL 18.6 values of `json[b]_to_record[set]` and `json[b]_populate_record[set]`.

use super::json_records_support::assert_rows;

#[test]
fn should_project_declared_columns_given_a_jsonb_object() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record('{\"a\":1,\"b\":\"x\",\"c\":2}') AS x(a int, b text)";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1"), Some("x")]]);
}

#[test]
fn should_return_null_for_a_missing_key_given_to_record() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record('{\"a\":1}') AS x(a int, b text)";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1"), None]]);
}

#[test]
fn should_keep_the_last_duplicate_key_given_json_input() {
    // Arrange
    let sql = "SELECT * FROM json_to_record('{\"a\":1,\"a\":2}') AS x(a int)";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("2")]]);
}

#[test]
fn should_match_keys_case_sensitively_given_to_record() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record('{\"A\":1}') AS x(a int)";
    // Act
    // Assert
    assert_rows(sql, &[&[None]]);
}

#[test]
fn should_return_one_null_row_given_a_null_document() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record(NULL) AS x(a int)";
    // Act
    // Assert
    assert_rows(sql, &[&[None]]);
}

#[test]
fn should_return_one_row_per_element_given_to_recordset() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_recordset('[{\"a\":1,\"b\":\"x\"},{\"a\":2}]') \
               AS x(a int, b text)";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1"), Some("x")], &[Some("2"), None]]);
}

#[test]
fn should_filter_recordset_rows_given_an_outer_predicate() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_recordset('[{\"a\":1,\"b\":\"x\"},{\"a\":2}]') \
               AS x(a int, b text) WHERE a > 1";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("2"), None]]);
}

#[test]
fn should_fill_missing_keys_from_the_base_given_populate_record() {
    // Arrange
    let sql = "SELECT * FROM jsonb_populate_record('(1,z)'::pair, '{\"a\":5}')";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("5"), Some("z")]]);
}

#[test]
fn should_fill_missing_keys_with_null_given_a_null_base() {
    // Arrange
    let sql = "SELECT * FROM jsonb_populate_record(NULL::pair, '{\"a\":5}')";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("5"), None]]);
}

#[test]
fn should_return_one_row_per_element_given_populate_recordset() {
    // Arrange
    let sql = "SELECT * FROM jsonb_populate_recordset(NULL::pair, '[{\"a\":5},{\"b\":\"q\"}]')";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("5"), None], &[None, Some("q")]]);
}

#[test]
fn should_number_rows_given_populate_recordset_with_ordinality() {
    // Arrange
    let sql = "SELECT * FROM jsonb_populate_recordset(NULL::pair, '[{\"a\":5},{\"b\":\"q\"}]') \
               WITH ORDINALITY";
    // Act
    // Assert
    assert_rows(
        sql,
        &[&[Some("5"), None, Some("1")], &[None, Some("q"), Some("2")]],
    );
}

#[test]
fn should_build_a_composite_field_given_a_nested_object() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record('{\"a\":{\"a\":1,\"b\":\"q\"}}') AS x(a pair)";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("(1,q)")]]);
}

#[test]
fn should_build_an_array_field_given_a_nested_array() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record('{\"a\":[1,2]}') AS x(a int[])";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("{1,2}")]]);
}

#[test]
fn should_keep_a_json_field_given_a_nested_object() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record('{\"a\":{\"k\":1}}') AS x(a jsonb)";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("{\"k\": 1}")]]);
}

#[test]
fn should_bind_per_row_documents_given_a_lateral_join() {
    // Arrange
    let sql = "SELECT t.id, r.a FROM (VALUES (1, '{\"a\":1}'::jsonb), (2, '{\"a\":2}'::jsonb)) \
               AS t(id, d), LATERAL jsonb_to_record(t.d) AS r(a int) ORDER BY t.id";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1"), Some("1")], &[Some("2"), Some("2")]]);
}

#[test]
fn should_return_sql_null_given_a_json_null_value_and_a_base() {
    // Arrange
    let sql = "SELECT * FROM jsonb_populate_record('(1,z)'::pair, '{\"a\":null}')";
    // Act
    // Assert
    assert_rows(sql, &[&[None, Some("z")]]);
}

#[test]
fn should_use_the_last_duplicate_key_given_populate_record_over_json() {
    // Arrange
    let sql = "SELECT * FROM json_populate_record(NULL::pair, '{\"a\":1,\"a\":2}')";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("2"), None]]);
}

#[test]
fn should_convert_a_json_number_given_a_text_column() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record('{\"a\":1.50}') AS x(a text)";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1.50")]]);
}

#[test]
fn should_convert_a_json_boolean_given_a_text_column() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record('{\"a\":true}') AS x(a text)";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("true")]]);
}

#[test]
fn should_keep_unmatched_left_rows_given_left_join_lateral_over_a_recordset() {
    // Arrange
    let sql = "SELECT t.id, r.a FROM (VALUES (1, '[{\"a\":5}]'::jsonb), (2, '[]'::jsonb)) \
               AS t(id, d) LEFT JOIN LATERAL jsonb_to_recordset(t.d) AS r(a int) ON true \
               ORDER BY t.id";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1"), Some("5")], &[Some("2"), None]]);
}

#[test]
fn should_build_a_two_dimensional_array_given_jsonb_array_elements_of_arrays() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record('{\"a\":[[1],[2]]}') AS x(a jsonb[])";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("{{1},{2}}")]]);
}

#[test]
fn should_keep_a_mixed_list_one_dimensional_given_a_jsonb_array_column() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record('{\"a\":[{\"k\":[1]},[2]]}') AS x(a jsonb[])";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("{\"{\\\"k\\\": [1]}\",[2]}")]]);
}

#[test]
fn should_ignore_trailing_blanks_given_a_varchar_column() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record('{\"a\":\"ab  \"}') AS x(a varchar(2))";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("ab")]]);
}

#[test]
fn should_return_an_empty_array_given_empty_sub_arrays() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record('{\"a\":[[],[]]}') AS x(a jsonb[])";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("{}")]]);
}

#[test]
fn should_build_two_dimensions_given_nested_json_elements() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record('{\"a\":[[1,[2]],[3,[4]]]}') AS x(a jsonb[])";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("{{1,[2]},{3,[4]}}")]]);
}
