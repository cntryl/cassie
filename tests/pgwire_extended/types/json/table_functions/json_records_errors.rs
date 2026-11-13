//! PostgreSQL 18.6 errors of the record-returning JSON sources.

use super::json_records_support::assert_failure;

#[test]
fn should_require_a_column_definition_list_given_to_record_without_one() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record('{\"a\":1}')";
    // Act
    // Assert
    assert_failure(sql, "42601", "a column definition list is required");
}

#[test]
fn should_reject_a_column_definition_list_given_populate_record() {
    // Arrange
    let sql = "SELECT * FROM jsonb_populate_record(NULL::pair, '{\"a\":1}') AS x(a int, b text)";
    // Act
    // Assert
    assert_failure(sql, "42601", "a column definition list is redundant");
}

#[test]
fn should_reject_duplicate_declared_columns_given_to_record() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record('{\"a\":1}') AS x(a int, a int)";
    // Act
    // Assert
    assert_failure(sql, "42701", "specified more than once");
}

#[test]
fn should_reject_with_ordinality_given_a_column_definition_list() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record('{\"a\":1}') WITH ORDINALITY AS x(a int)";
    // Act
    // Assert
    assert_failure(sql, "42601", "WITH ORDINALITY cannot be used");
}

#[test]
fn should_reject_a_non_conforming_value_given_to_record() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record('{\"a\":\"x\"}') AS x(a int)";
    // Act
    // Assert
    assert_failure(sql, "22P02", "invalid input syntax for type integer");
}

#[test]
fn should_reject_a_non_object_element_given_to_recordset() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_recordset('[1]') AS x(a int)";
    // Act
    // Assert
    assert_failure(sql, "22023", "must be an array of objects");
}

#[test]
fn should_reject_a_non_array_document_given_to_recordset() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_recordset('{\"a\":1}') AS x(a int)";
    // Act
    // Assert
    assert_failure(
        sql,
        "22023",
        "cannot call jsonb_to_recordset on a non-array",
    );
}

#[test]
fn should_reject_an_array_document_given_to_record() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record('[1]') AS x(a int)";
    // Act
    // Assert
    assert_failure(sql, "22023", "cannot call populate_composite on an array");
}

#[test]
fn should_reject_a_value_longer_than_varchar_given_to_record() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record('{\"a\":\"str\"}') AS x(a varchar(2))";
    // Act
    // Assert
    assert_failure(sql, "22001", "value too long");
}

#[test]
fn should_reject_ragged_json_array_columns_with_malformed_array() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record('{\"a\":[[1],[2,3]]}') AS x(a jsonb[])";
    // Act
    // Assert
    assert_failure(sql, "22P02", "malformed JSON array");
}

#[test]
fn should_reject_an_object_document_given_json_to_recordset_text_family() {
    // Arrange
    let sql = "SELECT * FROM json_to_recordset('{\"a\":1}') AS x(a int)";
    // Act
    // Assert
    assert_failure(sql, "22023", "cannot call json_to_recordset on an object");
}

#[test]
fn should_reject_a_scalar_document_given_json_to_recordset_text_family() {
    // Arrange
    let sql = "SELECT * FROM json_to_recordset('3') AS x(a int)";
    // Act
    // Assert
    assert_failure(sql, "22023", "cannot call json_to_recordset on a scalar");
}

#[test]
fn should_publish_no_rows_given_a_later_non_object_element() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_recordset('[{\"a\":1},3]') AS x(a int)";
    // Act
    // Assert
    assert_failure(sql, "22023", "must be an array of objects");
}

#[test]
fn should_reject_a_non_array_sibling_given_a_leading_sub_array() {
    // Arrange
    let sql = "SELECT * FROM jsonb_to_record('{\"a\":[[1],2]}') AS x(a jsonb[])";
    // Act
    // Assert
    assert_failure(sql, "22P02", "expected JSON array");
}
