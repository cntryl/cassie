use std::time::Instant;

use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

#[test]
fn should_match_owned_json_value_parse_and_retain_its_reservation() {
    // Arrange
    let inputs = [
        r#"{"b":1,"a":2,"b":3}"#,
        r#"["é", "\u0061", {"z":[null,true]}]"#,
        "123456789012345678901234567890.125",
        "1.2300e+5",
        "null",
    ];
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    // Act / Assert
    for text in inputs {
        let expected: serde_json::Value = serde_json::from_str(text).expect("original");
        let owned = super::parse(text.as_bytes(), &controls).expect("owned parse");
        assert_eq!(owned.get(), &expected);
        assert!(controls.current_query_memory_bytes() > 0);
        drop(owned);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_reject_invalid_json_and_utf8_without_retaining_memory() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let cases: &[&[u8]] = &[
        b"",
        b"{",
        b"true trailing",
        b"\xff",
        b"{bad\xff}",
        b"\"\\uD800\"",
    ];
    // Act / Assert
    for bytes in cases {
        assert!(super::parse(bytes, &controls).is_err());
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_admit_owned_json_at_exact_budget_and_deny_below() {
    // Arrange
    let input = br#"{"x":[1,2]}"#;
    let needed = super::super::serialized_decode_bytes(input.len()).expect("bound");
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: needed,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    // Act
    let owned = super::parse(input, &controls).expect("exact");
    // Assert
    assert_eq!(controls.current_query_memory_bytes(), needed);
    drop(owned);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: needed - 1,
        ..CassieRuntimeLimits::default()
    };
    let denied = QueryExecutionControls::from_limits(&limits, Instant::now());
    assert!(matches!(
        super::parse(input, &denied),
        Err(super::ParseError::Controlled(
            crate::app::CassieError::ResourceLimit(_)
        ))
    ));
    assert_eq!(denied.current_query_memory_bytes(), 0);
}

#[test]
fn should_interrupt_owned_json_during_actual_reader_consumption() {
    // Arrange
    let input = format!("\"{}\"", "a".repeat(2048));
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    // Act
    let result = super::parse_observed(input.as_bytes(), &controls, || cancellation.cancel());
    // Assert
    assert!(matches!(
        result,
        Err(super::ParseError::Controlled(
            crate::app::CassieError::QueryCancelled
        ))
    ));
    assert!(controls.peak_query_memory_bytes() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_report_cancellation_observed_while_serde_unwinds_a_prior_visitor_error() {
    // Arrange
    let prefix = r#"{"$serde_json::private::Number":"-xY""#;
    let mut input = String::from(prefix);
    input.push_str(&" ".repeat(1024));
    input.push('}');
    let expected =
        serde_json::from_str::<serde_json::Value>(&input).expect_err("number sentinel error");
    assert_eq!(expected.classify(), serde_json::error::Category::Data);
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );

    // Act
    let result = super::parse_observed(input.as_bytes(), &controls, || cancellation.cancel());

    // Assert
    assert!(matches!(
        result,
        Err(super::ParseError::Controlled(
            crate::app::CassieError::QueryCancelled
        ))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_validate_utf8_across_checked_input_boundaries() {
    // Arrange
    let text = format!("\"{}é\"", "a".repeat(4094));
    let expected: serde_json::Value = serde_json::from_str(&text).expect("original");
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    // Act
    let owned = super::parse(text.as_bytes(), &controls).expect("valid UTF-8");
    // Assert
    assert_eq!(owned.get(), &expected);
    drop(owned);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_json_error_owner_and_match_serde_diagnostic() {
    // Arrange
    let text = "{\"x\": [1,\n}";
    let expected = serde_json::from_str::<serde_json::Value>(text)
        .expect_err("invalid")
        .to_string();
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    // Act
    let error = super::parse(text.as_bytes(), &controls).expect_err("invalid");
    // Assert
    match &error {
        super::ParseError::InvalidJson { error, .. } => assert_eq!(error.to_string(), expected),
        other => panic!("unexpected error: {other:?}"),
    }
    assert!(controls.current_query_memory_bytes() > 0);
    drop(error);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_serde_private_keys_and_exponent_normalization() {
    // Arrange
    let inputs = [
        r"1E2",
        r"1e2",
        r"1e-02",
        r"-0",
        r#"{"$serde_json::private::Number":"123"}"#,
        r#"{"$serde_json::private::Number":"not-a-number"}"#,
        r#"{"$serde_json::private::RawValue":"{\"a\":1}"}"#,
        r#"{"$serde_json::private::RawValue":"{", "other": }"#,
    ];
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    // Act / Assert
    for text in inputs {
        let expected = serde_json::from_str::<serde_json::Value>(text);
        let actual = super::parse(text.as_bytes(), &controls);
        match (expected, actual) {
            (Ok(expected), Ok(actual)) => assert_eq!(actual.get(), &expected, "{text}"),
            (
                Err(expected),
                Err(super::ParseError::InvalidJson {
                    error,
                    _memory: memory,
                }),
            ) => {
                assert_eq!(error.to_string(), expected.to_string(), "{text}");
                drop(error);
                drop(memory);
            }
            (expected, actual) => panic!("mismatch for {text}: {expected:?}, {actual:?}"),
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
