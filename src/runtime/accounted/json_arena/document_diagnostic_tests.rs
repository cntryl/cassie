use std::time::Instant;

use super::super::{Document, DocumentError, Position, SyntaxKind};
use super::{render, DiagnosticError};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn controls(limit: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: limit,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

#[test]
fn should_retain_owned_row_diagnostic_and_match_legacy_parse_error_text() {
    // Arrange
    let controls = controls(1_048_576);
    let bytes = b"[}\xff";
    let expected = serde_json::from_slice::<serde_json::Value>(bytes)
        .expect_err("legacy")
        .to_string();
    let error = match Document::parse_plain_row(bytes, &controls) {
        Ok(_) => panic!("accepted invalid row"),
        Err(error) => error,
    };
    // Act
    let diagnostic = super::render_row(&error, &controls).expect("diagnostic");
    // Assert
    assert_eq!(
        diagnostic.as_str(),
        format!("invalid JSON in row blob: {expected}")
    );
    assert!(controls.current_query_memory_bytes() > 0);
    drop(diagnostic);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_row_diagnostic_after_actual_copy_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let error = match Document::parse_plain_row(b"[}", &controls) {
        Ok(_) => panic!("accepted invalid row"),
        Err(error) => error,
    };
    let mut copies = 0;
    // Act
    let result = super::render_row_observing(&error, &controls, || {
        copies += 1;
        cancel.cancel();
    });
    // Assert
    assert_eq!(copies, 1);
    assert!(matches!(
        result,
        Err(DiagnosticError::Control(CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_legacy_row_diagnostics_across_malformed_non_sentinel_inputs() {
    // Arrange
    let controls = controls(1_048_576);
    let inputs: &[&[u8]] = &[
        b"",
        b"[",
        b"[1,]",
        b"{\"a\":1,}",
        b"1e",
        b"{}\xff",
        b"{\"a\":\"\xff\\q\"}",
        b"{\"\xff\\q\":0}",
        b"\"\xff\"",
        b"{\n \"a\":0,\n \"\xff\\nX\":1}",
    ];
    // Act / Assert
    for bytes in inputs {
        let expected = serde_json::from_slice::<serde_json::Value>(bytes)
            .expect_err("legacy")
            .to_string();
        let error = match Document::parse_plain_row(bytes, &controls) {
            Ok(_) => panic!("accepted malformed row"),
            Err(error) => error,
        };
        let diagnostic = super::render_row(&error, &controls).expect("diagnostic");
        assert_eq!(
            diagnostic.as_str(),
            format!("invalid JSON in row blob: {expected}"),
            "{bytes:?}"
        );
        drop(diagnostic);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_render_original_jsonb_copy_diagnostic_for_serde_error_variants() {
    // Arrange
    let controls = controls(1_048_576);
    let inputs = [
        "",
        "[",
        "[1",
        "[1,]",
        "[1 2]",
        "[, ]",
        "{",
        "{\"a\"}",
        "{\"a\":",
        "{\"a\":1",
        "{\"a\":1 \"b\":2}",
        "{\"a\":1,}",
        "{,}",
        "-",
        "-x",
        "01",
        "1.",
        "truX",
        "1e+x",
        "\"",
        "\"\\u12\"",
        "\"\\u12G4\"",
        "\"\\uD800X\"",
        "\"\\uDC00\"",
        "\"a\n\"",
        r#"{"$serde_json::private::Number":"-xY"}"#,
        r#"{"$serde_json::private::Number":null}"#,
        r#"{"$serde_json::private::Number":true}"#,
        r#"{"$serde_json::private::Number":-9223372036854775808}"#,
        r#"{"$serde_json::private::Number":18446744073709551615}"#,
        r#"{"$serde_json::private::Number":18446744073709551616}"#,
        r#"{"$serde_json::private::Number":1.1}"#,
        r#"{"$serde_json::private::Number":[]}"#,
        r#"{"$serde_json::private::Number":{}}"#,
        r#"{"$serde_json::private::RawValue":null}"#,
        r#"{"$serde_json::private::RawValue":"{\"a\":]"}"#,
    ];

    // Act / Assert
    for input in inputs {
        let expected = serde_json::from_str::<serde_json::Value>(input).expect_err("invalid");
        let error = match Document::parse(input, &controls) {
            Ok(_) => panic!("accepted {input}"),
            Err(error) => error,
        };
        let actual = render(&error, 9, &controls).expect("diagnostic");
        assert_eq!(
            actual.as_str(),
            format!(
                "COPY binary field 9 is invalid: invalid input syntax for type jsonb: {expected}"
            ),
            "{input}"
        );
        drop(actual);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_render_nested_raw_and_recursion_diagnostics_with_inner_position() {
    // Arrange
    let controls = controls(1_048_576);
    let nested =
        r#"{"$serde_json::private::RawValue":"{\"$serde_json::private::RawValue\":\"[1,]\"}"}"#;
    let deep = format!("{}0{}", "[".repeat(128), "]".repeat(128));
    // Act / Assert
    for input in [nested, deep.as_str()] {
        let expected = serde_json::from_str::<serde_json::Value>(input).expect_err("invalid");
        let error = match Document::parse(input, &controls) {
            Ok(_) => panic!("accepted"),
            Err(error) => error,
        };
        let actual = render(&error, 1, &controls).expect("diagnostic");
        assert_eq!(
            actual.as_str(),
            format!(
                "COPY binary field 1 is invalid: invalid input syntax for type jsonb: {expected}"
            )
        );
        drop(actual);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_render_maximum_positions_and_column_without_unbounded_scratch() {
    // Arrange
    let controls = controls(1024);
    let position = Position {
        offset: usize::MAX,
        line: usize::MAX,
        column: usize::MAX,
    };
    let error = DocumentError::Syntax(SyntaxKind::ExpectedValue, position);
    // Act
    let result = render(&error, usize::MAX, &controls).expect("bounded diagnostic");
    // Assert
    assert_eq!(result.as_str(), format!("COPY binary field {} is invalid: invalid input syntax for type jsonb: expected value at line {} column {}", usize::MAX, usize::MAX, usize::MAX));
}

#[test]
fn should_admit_exact_diagnostic_and_release_on_denial_or_cancellation() {
    // Arrange
    let generous = controls(1024);
    let error = DocumentError::Syntax(
        SyntaxKind::EofValue,
        Position {
            offset: 0,
            line: 1,
            column: 0,
        },
    );
    let measured = render(&error, 1, &generous).expect("measure");
    let length = measured.as_str().len();
    drop(measured);
    let exact = controls(length);
    let below = controls(length - 1);
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();
    let cancelled = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation,
    );
    // Act / Assert
    let admitted = render(&error, 1, &exact).expect("exact");
    assert!(matches!(
        render(&error, 1, &below),
        Err(DiagnosticError::Control(CassieError::ResourceLimit(_)))
    ));
    assert!(matches!(
        render(&error, 1, &cancelled),
        Err(DiagnosticError::Control(CassieError::QueryCancelled))
    ));
    drop(admitted);
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert_eq!(below.current_query_memory_bytes(), 0);
}
