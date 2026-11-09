use std::time::Instant;

use super::{parse_field, JsonbFieldError};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_match_original_jsonb_canonical_text_for_valid_binary_fields() {
    // Arrange
    let controls = controls();
    let inputs = [
        "null",
        "-0",
        "1e+2",
        r#"{"abc":0,"é":1,"a":2,"a":3}"#,
        r#"{"$serde_json::private::RawValue":"[1,2]"}"#,
    ];
    // Act / Assert
    for input in inputs {
        let mut field = vec![1];
        field.extend_from_slice(input.as_bytes());
        let expected = crate::types::jsonb::canonical(input).expect("canonical");
        let output = parse_field(&field, 1, &controls).expect("field");
        assert_eq!(output.as_str(), expected);
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_version_utf8_parse_and_format_error_order() {
    // Arrange
    let controls = controls();
    let cases: &[(&[u8], &str)] = &[
        (&[], "invalid binary jsonb value"),
        (&[2, 255], "unsupported jsonb version number 2"),
        (&[1, 255], "invalid byte sequence for encoding UTF8"),
        (
            b"\x01[1,]",
            "invalid input syntax for type jsonb: trailing comma at line 1 column 4",
        ),
        (
            b"\x01{\"a\":1e+131072,\"b\":\"\\u0000\"}",
            "value overflows numeric format",
        ),
        (
            b"\x01{\"a\":\"\\u0000\",\"b\":1e+131072}",
            "unsupported Unicode escape sequence: \\u0000",
        ),
    ];
    // Act / Assert
    for (field, expected) in cases {
        let error = match parse_field(field, 2, &controls) {
            Ok(_) => panic!("accepted invalid field"),
            Err(error) => error,
        };
        assert!(matches!(&error, JsonbFieldError::Diagnostic(_)));
        assert_eq!(
            error.message(),
            Some(format!("COPY binary field 2 is invalid: {expected}").as_str())
        );
        drop(error);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_match_existing_jsonb_error_messages_for_nested_and_malformed_documents() {
    // Arrange
    let controls = controls();
    let inputs = [
        "[1,]",
        "{\"a\":",
        "\"\\u12G4\"",
        r#"{"$serde_json::private::Number":"-xY"}"#,
        r#"{"$serde_json::private::RawValue":"{\"a\":]"}"#,
        r#"{"a":"\u0000","b":]"#,
        r#"{"a":"\u0000","b":1e+131072}"#,
        r#"{"a":1e+131072,"b":"\u0000"}"#,
    ];
    // Act / Assert
    for input in inputs {
        let expected = crate::types::jsonb::canonical(input).expect_err("invalid JSONB");
        let mut field = vec![1];
        field.extend_from_slice(input.as_bytes());
        let error = match parse_field(&field, 1, &controls) {
            Ok(_) => panic!("accepted"),
            Err(error) => error,
        };
        assert_eq!(
            error.message(),
            Some(format!("COPY binary field 1 is invalid: {}", expected.message).as_str())
        );
        drop(error);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_keep_admitted_error_until_drop_and_propagate_cancellation() {
    // Arrange
    let controls = controls();
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();
    let cancelled = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation,
    );
    // Act
    let error = match parse_field(b"\x01[", 1, &controls) {
        Ok(_) => panic!("accepted"),
        Err(error) => error,
    };
    let admitted = controls.current_query_memory_bytes();
    let cancelled_result = parse_field(b"\x01null", 1, &cancelled);
    // Assert
    assert!(admitted > 0);
    assert!(matches!(
        cancelled_result,
        Err(JsonbFieldError::Control(CassieError::QueryCancelled))
    ));
    drop(error);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
