use std::time::Instant;

use super::super::{Cursor, CursorError, Position};
use super::{NumberDiagnostic, NumberError};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_normalize_value_numbers_like_the_pinned_serde_value() {
    // Arrange
    let controls = controls();
    let inputs = [
        "0",
        "-0",
        "1",
        "-1",
        "18446744073709551615",
        "18446744073709551616",
        "-9223372036854775808",
        "-9223372036854775809",
        "0.0",
        "-0.0",
        "1E2",
        "1e-0003",
        "1e999999999999999999999999999999999999",
    ];

    // Act / Assert
    for input in inputs {
        let expected = serde_json::from_str::<serde_json::Value>(input)
            .expect("serde")
            .to_string();
        let mut cursor = Cursor::from_str(input, &controls);
        let value = cursor.read_number().expect("number");
        // Assert
        assert_eq!(value.as_str(), expected, "{input}");
        assert_eq!(cursor.peek(), None);
        drop(value);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_match_value_number_errors_against_serde() {
    // Arrange
    let controls = controls();
    let inputs = ["01", "-", "-xY", "1.", "1.x", "1e", "1e+", "1e+x", "1e+xZ"];

    // Act / Assert
    for input in inputs {
        let expected = serde_json::from_str::<serde_json::Value>(input).expect_err("serde error");
        let mut cursor = Cursor::from_str(input, &controls);
        let actual = cursor.read_number();
        let Err(CursorError::Number(NumberDiagnostic { kind, position })) = actual else {
            panic!("expected number error for {input:?}");
        };
        let category = if kind == NumberError::Eof {
            serde_json::error::Category::Eof
        } else {
            serde_json::error::Category::Syntax
        };
        // Assert
        assert_eq!(expected.classify(), category, "{input:?}");
        assert_eq!(
            (expected.line(), expected.column()),
            (position.line, position.column),
            "{input:?}"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_leave_delimiters_in_value_mode_with_reject_them_in_strict_mode() {
    // Arrange
    let controls = controls();
    let mut prefix = Cursor::from_str("1E2,", &controls);
    let mut strict = Cursor::from_str("1 ", &controls);
    let mut override_error = Cursor::from_str("-xY", &controls);

    // Act / Assert
    let value = prefix.read_number().expect("prefix");
    // Assert
    assert_eq!(value.as_str(), "1e+2");
    assert_eq!(prefix.peek(), Some(b','));
    assert!(matches!(
        strict.read_strict_number(),
        Err(CursorError::Number(NumberDiagnostic {
            kind: NumberError::Invalid,
            position: Position {
                line: 1,
                column: 2,
                ..
            }
        }))
    ));
    let expected = "-xY"
        .parse::<serde_json::Number>()
        .expect_err("serde strict");
    let Err(CursorError::Number(error)) = override_error.read_strict_number() else {
        panic!("strict error");
    };
    assert_eq!(
        (error.position.line, error.position.column),
        (expected.line(), expected.column())
    );
    assert_eq!(error.position.column, 3);
    drop(value);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_strict_number_errors_with_override_positions_against_serde() {
    // Arrange
    let controls = controls();
    let inputs = ["", "-", "-xY", "1 ", "1e+xZ", "\n", "01"];

    // Act / Assert
    for input in inputs {
        let expected = input
            .parse::<serde_json::Number>()
            .expect_err("serde strict error");
        let mut cursor = Cursor::from_str(input, &controls);
        let Err(CursorError::Number(error)) = cursor.read_strict_number() else {
            panic!("strict error for {input:?}");
        };
        let category = if error.kind == NumberError::Eof {
            serde_json::error::Category::Eof
        } else {
            serde_json::error::Category::Syntax
        };
        // Assert
        assert_eq!(expected.classify(), category, "{input:?}");
        assert_eq!(
            (expected.line(), expected.column()),
            (error.position.line, error.position.column),
            "{input:?}"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_admit_number_buffers_with_release_them_on_growth_failure() {
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
    let mut first = Cursor::from_str("12345", &exact);
    let mut second = Cursor::from_str("12345", &below);

    // Act / Assert
    let value = first.read_number().expect("exact");
    // Assert
    assert_eq!(value.as_str(), "12345");
    assert!(matches!(
        second.read_number(),
        Err(CursorError::Interrupted(CassieError::ResourceLimit(_)))
    ));
    assert_eq!(below.current_query_memory_bytes(), 0);
    drop(value);
    assert_eq!(exact.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_during_number_buffer_growth_with_release_both_owners() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut cursor = Cursor::from_str("12345", &controls);
    let mut saw_overlap = false;

    // Act
    let result = cursor.read_number_observed(|| {
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
fn should_never_return_a_number_token_for_invalid_or_non_numeric_input() {
    // Arrange
    let controls = controls();

    // Act / Assert
    for input in ["-", "01", "1e+", "true", "\"1\""] {
        let mut cursor = Cursor::from_str(input, &controls);
        // Assert
        assert!(
            matches!(cursor.read_number(), Err(CursorError::Number(_))),
            "{input:?}"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
