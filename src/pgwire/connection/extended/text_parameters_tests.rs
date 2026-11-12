use std::time::Instant;

use super::decode_text_parameter;
use crate::config::CassieRuntimeLimits;
use crate::pgwire::connection::extended::ExtendedQueryError;
use crate::runtime::QueryExecutionControls;
use crate::types::Value;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_decode_text_json_reserved_keys_as_objects() {
    // Arrange
    let controls = controls();
    let input = br#"{"$serde_json::private::Number":"7","nested":{"$serde_json::private::RawValue":"[1,]"}}"#;

    // Act
    let decoded = decode_text_parameter(input, 114, &controls).expect("JSON bind");

    // Assert
    let Value::Json(value) = &decoded.value else {
        panic!("JSON bind should retain the JSON value");
    };
    assert_eq!(
        value["$serde_json::private::Number"],
        serde_json::Value::String("7".into())
    );
    assert_eq!(
        value["nested"]["$serde_json::private::RawValue"],
        serde_json::Value::String("[1,]".into())
    );
}

#[test]
fn should_retain_text_json_bind_reservation_until_the_decoded_value_drops() {
    // Arrange
    let controls = controls();
    let input = br#"{"key":"value"}"#;

    // Act
    let decoded = decode_text_parameter(input, 114, &controls).expect("JSON bind");

    // Assert
    assert!(decoded.memory.is_some());
    assert!(controls.current_query_memory_bytes() > 0);
    drop(decoded);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_validate_jsonb_reserved_key_objects_without_sentinel_conversion() {
    // Arrange
    let controls = controls();
    let input = br#"{"$serde_json::private::Number":"not-a-number","n":1.2300e+5}"#;

    // Act
    let decoded = decode_text_parameter(input, 3802, &controls).expect("JSONB bind");

    // Assert
    let Value::Json(value) = &decoded.value else {
        panic!("JSONB bind should retain the JSON value");
    };
    assert_eq!(
        value["$serde_json::private::Number"],
        serde_json::Value::String("not-a-number".into())
    );
    assert_eq!(
        value["n"].as_number().expect("number").as_str(),
        "1.2300e+5"
    );
    assert!(decoded.memory.is_some());
    drop(decoded);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_json_bind_reservation_when_jsonb_numeric_validation_fails() {
    // Arrange
    let controls = controls();

    // Act
    let result = decode_text_parameter(b"1e+131072", 3802, &controls);

    // Assert
    assert!(matches!(
        result,
        Err(ExtendedQueryError {
            pg_error,
            ..
        }) if pg_error.code == "22003"
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_json_bind_syntax_errors_distinct_from_control_failures() {
    // Arrange
    let controls = controls();

    // Act
    let result = decode_text_parameter(b"[1,]", 114, &controls);

    // Assert
    assert!(matches!(
        result,
        Err(ExtendedQueryError {
            pg_error,
            ..
        }) if pg_error.message == "invalid JSON bind parameter"
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_report_json_bind_memory_limit_as_a_controlled_error() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 0,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());

    // Act
    let result = decode_text_parameter(b"null", 114, &controls);

    // Assert
    assert!(matches!(
        result,
        Err(ExtendedQueryError {
            pg_error,
            ..
        }) if pg_error.code == "54000"
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_jsonb_nul_validation_for_ordinary_objects() {
    // Arrange
    let controls = controls();
    let input = br#"{"nested":"\u0000"}"#;

    // Act
    let result = decode_text_parameter(input, 3802, &controls);

    // Assert
    assert!(matches!(
        result,
        Err(ExtendedQueryError {
            pg_error,
            ..
        }) if pg_error.code == "22P05"
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_report_jsonb_syntax_errors_with_type_input_sqlstate() {
    // Arrange
    let controls = controls();

    // Act
    let result = decode_text_parameter(b"[1,]", 3802, &controls);

    // Assert
    assert!(matches!(
        result,
        Err(ExtendedQueryError {
            pg_error,
            ..
        }) if pg_error.code == "22P02"
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_json_parse_control_error_as_the_original_cassie_error() {
    // Arrange
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    cancellation.cancel();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation,
    );

    // Act
    let result = decode_text_parameter(b"null", 114, &controls);

    // Assert
    assert!(matches!(
        result,
        Err(ExtendedQueryError {
            pg_error,
            ..
        }) if pg_error.code == "57014"
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_charge_temporal_text_bind_scratch_at_the_exact_query_budget() {
    // Arrange
    let input = b"2024-01-15 12:00:00 UTC";
    let required = crate::types::extended::temporal::tokens::tokenization_memory_bound(
        std::str::from_utf8(input).expect("temporal input"),
    )
    .expect("temporal scratch bound");
    let exact = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: required,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let below = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: required - 1,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );

    // Act
    let accepted = decode_text_parameter(input, 1184, &exact);
    let rejected = decode_text_parameter(input, 1184, &below);

    // Assert
    assert!(accepted.is_ok());
    assert!(matches!(
        rejected,
        Err(ExtendedQueryError { pg_error, .. }) if pg_error.code == "54000"
    ));
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert_eq!(below.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_query_cancellation_for_temporal_text_bind() {
    // Arrange
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();

    // Act
    let result = decode_text_parameter(b"2024-01-15 12:00:00 UTC", 1184, &controls);

    // Assert
    assert!(matches!(
        result,
        Err(ExtendedQueryError { pg_error, .. }) if pg_error.code == "57014"
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
