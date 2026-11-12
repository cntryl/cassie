//! Deferred SQL/JSON forms reject with stable SQLSTATEs instead of panicking (#810).

use super::{fixture, wire};

const SYNTAX_ERROR: &str = "42601";
const UNDEFINED_OBJECT: &str = "42704";

/// Runs `sql`, then a follow-up query on the same connection, and returns
/// the rejection SQLSTATE plus the follow-up rows.
fn run_then_recover(label: &str, sql: &str) -> (Option<String>, Vec<Vec<Option<String>>>) {
    let cycles = vec![
        fixture::execute_cycle(sql, None, 0, true),
        fixture::execute_cycle("SELECT 7::INT", None, 0, true),
    ];
    let batches = fixture::run_cycles(label, &[], cycles);
    fixture::assert_success(&batches[1]);
    (wire::error_code(&batches[0]), wire::data_rows(&batches[1]))
}

#[test]
fn should_reject_row_to_json_as_an_unsupported_function() {
    // Arrange
    let sql = "SELECT row_to_json(ROW(1, 2))";

    // Act
    let (code, recovery) = run_then_recover("deferred-row-to-json", sql);

    // Assert
    assert_eq!(code.as_deref(), Some(SYNTAX_ERROR));
    assert_eq!(recovery, vec![vec![Some("7".into())]]);
}

#[test]
fn should_reject_array_to_json_as_an_unsupported_function() {
    // Arrange
    let sql = "SELECT array_to_json(ARRAY[1, 2])";

    // Act
    let (code, recovery) = run_then_recover("deferred-array-to-json", sql);

    // Assert
    assert_eq!(code.as_deref(), Some(SYNTAX_ERROR));
    assert_eq!(recovery, vec![vec![Some("7".into())]]);
}

#[test]
fn should_reject_is_json_predicates() {
    // Arrange
    let sql = "SELECT '{}' IS JSON";

    // Act
    let (code, recovery) = run_then_recover("deferred-is-json", sql);

    // Assert
    assert_eq!(code.as_deref(), Some(SYNTAX_ERROR));
    assert_eq!(recovery, vec![vec![Some("7".into())]]);
}

#[test]
fn should_reject_typed_is_json_predicates() {
    // Arrange
    let sql = "SELECT '{}'::JSONB IS JSON";

    // Act
    let (code, recovery) = run_then_recover("deferred-is-json-typed", sql);

    // Assert
    assert_eq!(code.as_deref(), Some(UNDEFINED_OBJECT));
    assert_eq!(recovery, vec![vec![Some("7".into())]]);
}

#[test]
fn should_reject_json_value() {
    // Arrange
    let sql = "SELECT JSON_VALUE('{\"a\":1}', '$.a')";

    // Act
    let (code, recovery) = run_then_recover("deferred-json-value", sql);

    // Assert
    assert_eq!(code.as_deref(), Some(SYNTAX_ERROR));
    assert_eq!(recovery, vec![vec![Some("7".into())]]);
}

#[test]
fn should_reject_json_query() {
    // Arrange
    let sql = "SELECT JSON_QUERY('{\"a\":1}', '$.a')";

    // Act
    let (code, recovery) = run_then_recover("deferred-json-query", sql);

    // Assert
    assert_eq!(code.as_deref(), Some(SYNTAX_ERROR));
    assert_eq!(recovery, vec![vec![Some("7".into())]]);
}

#[test]
fn should_reject_the_passing_clause() {
    // Arrange
    let sql = "SELECT JSON_VALUE('{\"a\":1}', '$.a' PASSING 1 AS x)";

    // Act
    let (code, recovery) = run_then_recover("deferred-passing", sql);

    // Assert
    assert_eq!(code.as_deref(), Some(SYNTAX_ERROR));
    assert_eq!(recovery, vec![vec![Some("7".into())]]);
}
