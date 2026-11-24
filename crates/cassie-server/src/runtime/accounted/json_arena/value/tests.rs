use std::time::Instant;

use super::{parse_value, ValueParseError};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_preserve_number_looking_key_as_an_ordinary_json_field() {
    // Arrange
    let controls = controls();
    let input = br#"{"$serde_json::private::Number":"ordinary"}"#;

    // Act
    let actual = parse_value(input, &controls).expect("ordinary JSON parse");

    // Assert
    assert_eq!(
        actual.get(),
        &serde_json::json!({"$serde_json::private::Number": "ordinary"})
    );
}

#[test]
fn should_preserve_raw_value_looking_key_as_an_ordinary_json_field() {
    // Arrange
    let controls = controls();
    let input = br#"{"$serde_json::private::RawValue":"[1,]"}"#;

    // Act
    let actual = parse_value(input, &controls).expect("ordinary JSON parse");

    // Assert
    assert_eq!(
        actual.get(),
        &serde_json::json!({"$serde_json::private::RawValue": "[1,]"})
    );
}

#[test]
fn should_preserve_reserved_looking_keys_inside_nested_objects() {
    // Arrange
    let controls = controls();
    let input = br#"{"nested":{"$serde_json::private::Number":"not-a-number","$serde_json::private::RawValue":"[1,]"}}"#;

    // Act
    let actual = parse_value(input, &controls).expect("ordinary JSON parse");

    // Assert
    assert_eq!(
        actual.get(),
        &serde_json::json!({"nested": {"$serde_json::private::Number": "not-a-number", "$serde_json::private::RawValue": "[1,]"}})
    );
}

#[test]
fn should_resolve_escaped_duplicate_object_keys_before_value_materialization() {
    // Arrange
    let controls = controls();
    let input = br#"{"$serde_json::private::Number":"first","$serde_json\u003a\u003aprivate::Number":"last"}"#;

    // Act
    let actual = parse_value(input, &controls).expect("ordinary JSON parse");

    // Assert
    assert_eq!(
        actual.get(),
        &serde_json::json!({"$serde_json::private::Number": "last"})
    );
}

#[test]
fn should_preserve_exact_json_number_lexeme_during_materialization() {
    // Arrange
    let controls = controls();
    let input = br#"{"exact":1.2300e+5}"#;

    // Act
    let actual = parse_value(input, &controls).expect("ordinary JSON parse");

    // Assert
    assert_eq!(
        actual.get()["exact"].as_number().expect("number").as_str(),
        "1.2300e+5"
    );
}

#[test]
fn should_retain_json_parse_reservation_until_the_materialized_value_drops() {
    // Arrange
    let controls = controls();
    let input =
        br#"{"$serde_json::private::Number":"ordinary","$serde_json::private::RawValue":"[1,]"}"#;

    // Act
    let actual = parse_value(input, &controls).expect("ordinary JSON parse");

    // Assert
    assert!(actual.accounted_bytes() >= input.len());
    assert!(controls.current_query_memory_bytes() > 0);
    drop(actual);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_json_parse_reservations_after_invalid_input() {
    // Arrange
    let controls = controls();

    // Act
    let result = parse_value(b"[1,]", &controls);

    // Assert
    assert!(matches!(result, Err(ValueParseError::Invalid)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_json_materialization_before_parsing_when_its_bound_exceeds_budget() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 0,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());

    // Act
    let result = parse_value(b"null", &controls);

    // Assert
    assert!(matches!(
        result,
        Err(ValueParseError::Controlled(CassieError::ResourceLimit(_)))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_json_materialization_reservation_when_canceled() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();

    // Act
    let result = parse_value(br#"{"key":"value"}"#, &controls);

    // Assert
    assert!(matches!(
        result,
        Err(ValueParseError::Controlled(CassieError::QueryCanceled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
