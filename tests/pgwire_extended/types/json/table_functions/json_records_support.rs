//! Shared wire observations for the record-returning and `JSON_TABLE` FROM sources.

use super::json_elements_support::{error_message, query, Rows};
use super::wire;
use cassie::types::Value;

fn pair_setup() -> Vec<(&'static str, Vec<Value>)> {
    vec![("CREATE TYPE pair AS (a INT, b TEXT)", Vec::new())]
}

/// Data rows of one statement over a fresh fixture that defines the composite `pair`.
pub(super) fn rows(sql: &str) -> Rows {
    wire::data_rows(&query("json-records", &pair_setup(), sql, false))
}

/// SQLSTATE and message of a statement that must fail.
pub(super) fn failure(sql: &str) -> (Option<String>, Option<String>) {
    let frames = query("json-records-failure", &pair_setup(), sql, false);
    (wire::error_code(&frames), error_message(&frames))
}

/// Asserts the rows of `sql`, given as text cells with `None` for SQL NULL.
pub(super) fn assert_rows(sql: &str, expected: &[&[Option<&str>]]) {
    let expected = expected
        .iter()
        .map(|cells| super::json_elements_support::row(cells))
        .collect::<Rows>();
    assert_eq!(rows(sql), expected, "{sql}");
}

/// Asserts that `sql` fails with `state` and a message containing `fragment`.
pub(super) fn assert_failure(sql: &str, state: &str, fragment: &str) {
    let (code, message) = failure(sql);
    assert_eq!(code.as_deref(), Some(state), "{sql}");
    let message = message.unwrap_or_default();
    assert!(message.contains(fragment), "{sql}: {message}");
}

/// Data rows over tables `lf_outer(id)` and `lf_inner(id, d)` for correlated lateral cases.
pub(super) fn rows_over_lateral_tables(sql: &str) -> Rows {
    let setup = vec![
        ("CREATE TABLE lf_outer (id INT)", Vec::new()),
        ("INSERT INTO lf_outer VALUES (1), (2)", Vec::new()),
        ("CREATE TABLE lf_inner (id INT, d JSONB)", Vec::new()),
        (
            "INSERT INTO lf_inner VALUES (1, '[10]'), (2, '[20]')",
            Vec::new(),
        ),
    ];
    wire::data_rows(&query("json-lateral-tables", &setup, sql, false))
}

/// Asserts the rows of `sql` over the lateral fixture tables.
pub(super) fn assert_lateral_rows(sql: &str, expected: &[&[Option<&str>]]) {
    let expected = expected
        .iter()
        .map(|cells| super::json_elements_support::row(cells))
        .collect::<Rows>();
    assert_eq!(rows_over_lateral_tables(sql), expected, "{sql}");
}
