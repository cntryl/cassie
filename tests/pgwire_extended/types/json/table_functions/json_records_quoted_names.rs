//! Quoted column names read the JSON key spelled by the identifier (#829).
//! Expected rows come from PostgreSQL 18.6.

use super::json_records_support::assert_rows;

#[test]
fn should_read_a_camel_case_key_given_a_quoted_to_record_column() {
    // Arrange
    let sql = "SELECT * FROM json_to_record('{\"userId\":7,\"userid\":8}') AS t(\"userId\" int)";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("7")]]);
}

#[test]
fn should_select_a_quoted_column_by_name_given_jsonb_to_record() {
    // Arrange
    let sql = "SELECT \"userId\" FROM jsonb_to_record('{\"userId\":7}') AS t(\"userId\" int)";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("7")]]);
}

#[test]
fn should_read_each_quoted_key_spelling_given_a_quoted_recordset_list() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_recordset(\
               '[{\"first name\":\"a\",\"a.b\":1,\"Up\":2,\"up\":3}]') \
               AS t(\"first name\" text, \"a.b\" int, \"Up\" int, up int)";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("a"), Some("1"), Some("2"), Some("3")]]);
}

#[test]
fn should_read_a_mixed_case_key_given_a_quoted_json_table_column() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('[{\"Abc\":1,\"abc\":2}]'::jsonb, '$[*]' \
               COLUMNS (\"Abc\" int, abc int)) AS t";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1"), Some("2")]]);
}

#[test]
fn should_read_a_spaced_key_given_a_quoted_json_table_column() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('[{\"a b\":1}]'::jsonb, '$[*]' \
               COLUMNS (\"a b\" int, found bool EXISTS PATH '$.\"a b\"')) AS t";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1"), Some("t")]]);
}
