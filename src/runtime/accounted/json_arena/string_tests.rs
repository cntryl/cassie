use std::time::Instant;

use super::super::{Cursor, CursorError, Position};
use super::{Diagnostic, ErrorKind};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_decode_value_string_escapes_and_surrogate_pairs() {
    // Arrange
    let controls = controls();
    let mut cursor = Cursor::from_str(r#""\/\"\\\b\f\n\r\t\uD83D\uDE42""#, &controls);

    // Act
    let value = cursor.read_string().expect("string");

    // Assert
    assert_eq!(value.as_str(), "/\"\\\u{8}\u{c}\n\r\t🙂");
    assert_eq!(cursor.peek(), None);
    drop(value);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_value_string_error_kinds_and_consumed_positions() {
    // Arrange
    let controls = controls();
    let cases = [
        ("\"a\n\"", ErrorKind::Control, 3, 2, 0),
        ("\"\\u12G4\"", ErrorKind::InvalidEscape, 7, 1, 7),
        ("\"\\u12\"", ErrorKind::EofString, 6, 1, 6),
        ("\"\\uDC00\"", ErrorKind::LoneSurrogate, 7, 1, 7),
        ("\"\\uD800X\"", ErrorKind::UnexpectedHexEnd, 8, 1, 8),
        ("\"\\uD800\\u0041\"", ErrorKind::LoneSurrogate, 13, 1, 13),
        ("\"\\z\"", ErrorKind::InvalidEscape, 3, 1, 3),
        ("\"\\uD800", ErrorKind::EofString, 7, 1, 7),
        ("\"unterminated", ErrorKind::EofString, 13, 1, 13),
    ];

    // Act / Assert
    for (input, kind, offset, line, column) in cases {
        let mut cursor = Cursor::from_str(input, &controls);
        assert!(
            matches!(cursor.read_string(), Err(CursorError::Syntax(Diagnostic { kind: actual, position: Position { offset: at, line: row, column: col } })) if actual == kind && at == offset && row == line && col == column),
            "{input:?}"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
        let serde_error =
            serde_json::from_str::<serde_json::Value>(input).expect_err("serde error");
        let expected_category = if kind == ErrorKind::EofString {
            serde_json::error::Category::Eof
        } else {
            serde_json::error::Category::Syntax
        };
        assert_eq!(serde_error.classify(), expected_category, "{input:?}");
        assert_eq!(
            (serde_error.line(), serde_error.column()),
            (line, column),
            "{input:?}"
        );
    }
}

#[test]
fn should_admit_string_capacity_and_release_on_denial() {
    // Arrange
    let exact = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 4,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let below = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 3,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let mut exact_cursor = Cursor::from_str("\"abc\"", &exact);
    let mut below_cursor = Cursor::from_str("\"abc\"", &below);

    // Act / Assert
    let value = exact_cursor.read_string().expect("exact capacity");
    assert_eq!(value.as_str(), "abc");
    assert_eq!(exact.current_query_memory_bytes(), 4);
    assert!(matches!(
        below_cursor.read_string(),
        Err(CursorError::Interrupted(CassieError::ResourceLimit(_)))
    ));
    assert_eq!(below.current_query_memory_bytes(), 0);
    drop(value);
    assert_eq!(exact.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_partial_string_after_actual_copy_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut cursor = Cursor::from_str("\"hello\"", &controls);

    // Act
    let result = cursor.read_string_observed(|| cancellation.cancel());

    // Assert
    assert!(matches!(
        result,
        Err(CursorError::Interrupted(CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_observe_cancellation_before_inspecting_a_string_token() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut cursor = Cursor::from_str("x", &controls);
    cancellation.cancel();

    // Act / Assert
    assert!(matches!(
        cursor.read_string(),
        Err(CursorError::Interrupted(CassieError::QueryCancelled))
    ));
    assert_eq!(
        cursor.position(),
        Position {
            offset: 0,
            line: 1,
            column: 0
        }
    );
}

#[test]
fn should_release_both_string_owners_when_cancelled_during_buffer_growth() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut cursor = Cursor::from_str("\"abcde\"", &controls);
    let mut saw_overlap = false;

    // Act
    let result = cursor.read_string_observed(|| {
        if controls.current_query_memory_bytes() == 12 {
            saw_overlap = true;
            cancellation.cancel();
        }
    });

    // Assert
    assert!(saw_overlap);
    assert!(matches!(
        result,
        Err(CursorError::Interrupted(CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_both_string_buffers_during_growth() {
    // Arrange
    let exact = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 12,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let below = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 11,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let mut exact_cursor = Cursor::from_str("\"abcde\"", &exact);
    let mut below_cursor = Cursor::from_str("\"abcde\"", &below);

    // Act / Assert
    let value = exact_cursor.read_string().expect("old and new admitted");
    assert_eq!(value.as_str(), "abcde");
    assert_eq!(exact.current_query_memory_bytes(), 8);
    assert!(matches!(
        below_cursor.read_string(),
        Err(CursorError::Interrupted(CassieError::ResourceLimit(_)))
    ));
    assert_eq!(below.current_query_memory_bytes(), 0);
    drop(value);
    assert_eq!(exact.current_query_memory_bytes(), 0);
}
