//! Output labels without `AS` on the default columns of JSON FROM sources (#829).
//! Expected rows and names come from PostgreSQL 18.6.

use super::json_elements_support::{column_names, error_message, query, row, Rows};
use super::wire;

fn labelled(sql: &str) -> (Vec<String>, Rows) {
    let frames = query("json-source-labels", &[], sql, true);
    assert_eq!(error_message(&frames), None, "{sql}");
    (column_names(&frames), wire::data_rows(&frames))
}

#[test]
fn should_label_the_value_column_given_a_bare_label() {
    // Arrange
    let sql = "SELECT value v FROM jsonb_array_elements('[1]')";
    // Act
    let (names, rows) = labelled(sql);
    // Assert
    assert_eq!(names, vec!["v".to_owned()]);
    assert_eq!(rows, vec![row(&[Some("1")])]);
}

#[test]
fn should_label_both_member_columns_given_bare_labels() {
    // Arrange
    let sql = "SELECT key k, value v FROM jsonb_each_text('{\"a\":\"x\"}')";
    // Act
    let (names, rows) = labelled(sql);
    // Assert
    assert_eq!(names, vec!["k".to_owned(), "v".to_owned()]);
    assert_eq!(rows, vec![row(&[Some("a"), Some("x")])]);
}

#[test]
fn should_label_a_qualified_column_given_a_bare_label() {
    // Arrange
    let sql = "SELECT x.a v, x.b FROM jsonb_to_recordset('[{\"a\":1,\"b\":\"q\"}]') \
               AS x(a int, b text)";
    // Act
    let (names, rows) = labelled(sql);
    // Assert
    assert_eq!(names, vec!["v".to_owned(), "b".to_owned()]);
    assert_eq!(rows, vec![row(&[Some("1"), Some("q")])]);
}

#[test]
fn should_keep_the_case_of_a_quoted_bare_label() {
    // Arrange
    let sql = "SELECT value \"V v\" FROM jsonb_array_elements('[1]')";
    // Act
    let (names, rows) = labelled(sql);
    // Assert
    assert_eq!(names, vec!["V v".to_owned()]);
    assert_eq!(rows, vec![row(&[Some("1")])]);
}
