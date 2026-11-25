//! Extended-protocol parameters for the record-returning and `JSON_TABLE` FROM sources.

use super::json_elements_support::{error_message, row, Rows};
use super::{fixture, wire};

/// Runs one Parse/Bind/Execute cycle with a single text-format parameter (`None` is SQL NULL).
fn bound(sql: &str, parameter: Option<&str>) -> fixture::Frames {
    let body = parameter.map(str::as_bytes);
    let cycle = fixture::execute_cycle(sql, Some((0, 0, body)), 0, false);
    fixture::run_cycles("json-records-parameters", &[], vec![cycle]).remove(0)
}

fn rows_of(sql: &str, parameter: Option<&str>) -> Rows {
    wire::data_rows(&bound(sql, parameter))
}

const RECORDSET: &str = "SELECT item.n FROM jsonb_to_recordset($1::jsonb) AS item(n bigint)";
const TABLE: &str = "SELECT * FROM JSON_TABLE($1::jsonb, '$[*]' COLUMNS (n int PATH '$.n'))";

#[test]
fn should_read_declared_columns_given_a_bound_recordset_parameter() {
    // Arrange
    let parameter = Some(r#"[{"n":1},{"n":2}]"#);
    // Act
    let result = rows_of(RECORDSET, parameter);
    // Assert
    assert_eq!(result, vec![row(&[Some("1")]), row(&[Some("2")])]);
}

#[test]
fn should_return_no_rows_given_a_null_recordset_parameter() {
    // Arrange
    let parameter = None;
    // Act
    let frames = bound(RECORDSET, parameter);
    // Assert
    assert_eq!(error_message(&frames), None);
    assert_eq!(wire::data_rows(&frames), Rows::new());
}

#[test]
fn should_reject_malformed_json_given_a_bound_recordset_parameter() {
    // Arrange
    let parameter = Some("[1,");
    // Act
    let frames = bound(RECORDSET, parameter);
    // Assert
    assert_eq!(wire::error_code(&frames).as_deref(), Some("22P02"));
    assert_eq!(wire::data_rows(&frames), Rows::new());
}

#[test]
fn should_reject_a_non_array_document_given_a_bound_recordset_parameter() {
    // Arrange
    let parameter = Some(r#"{"n":1}"#);
    // Act
    let frames = bound(RECORDSET, parameter);
    // Assert
    assert_eq!(wire::error_code(&frames).as_deref(), Some("22023"));
}

#[test]
fn should_expand_elements_given_an_unspecified_type_array_elements_parameter() {
    // Arrange
    let sql = "SELECT value FROM jsonb_array_elements($1)";
    // Act
    let result = rows_of(sql, Some(r#"[1,"a"]"#));
    // Assert
    assert_eq!(result, vec![row(&[Some("1")]), row(&[Some("\"a\"")])]);
}

#[test]
fn should_expand_members_given_an_unspecified_type_each_parameter() {
    // Arrange
    let sql = "SELECT key, value FROM jsonb_each($1)";
    // Act
    let result = rows_of(sql, Some(r#"{"a":1,"b":[2]}"#));
    // Assert
    assert_eq!(
        result,
        vec![row(&[Some("a"), Some("1")]), row(&[Some("b"), Some("[2]")])]
    );
}

#[test]
fn should_reject_an_object_given_an_array_elements_parameter() {
    // Arrange
    let sql = "SELECT value FROM jsonb_array_elements($1)";
    // Act
    let frames = bound(sql, Some(r#"{"a":1}"#));
    // Assert
    assert_eq!(wire::error_code(&frames).as_deref(), Some("22023"));
}

#[test]
fn should_project_columns_given_a_bound_json_table_context() {
    // Arrange
    let parameter = Some(r#"[{"n":4},{"n":5}]"#);
    // Act
    let result = rows_of(TABLE, parameter);
    // Assert
    assert_eq!(result, vec![row(&[Some("4")]), row(&[Some("5")])]);
}

#[test]
fn should_return_no_rows_given_a_null_json_table_context() {
    // Arrange
    let parameter = None;
    // Act
    let frames = bound(TABLE, parameter);
    // Assert
    assert_eq!(error_message(&frames), None);
    assert_eq!(wire::data_rows(&frames), Rows::new());
}

#[test]
fn should_reject_malformed_json_given_a_bound_json_table_context() {
    // Arrange
    let parameter = Some("[{");
    // Act
    let frames = bound(TABLE, parameter);
    // Assert
    assert_eq!(wire::error_code(&frames).as_deref(), Some("22P02"));
}
