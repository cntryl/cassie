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
