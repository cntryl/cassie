use super::{decode, decode_at, decode_observing, RawPosition};
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn controls(cancel: QueryCancellationHandle) -> QueryExecutionControls {
    QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel,
    )
}

#[test]
fn should_match_serde_decoding_for_reached_raw_strings() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let values = [
        r#""hello""#,
        r#""é\n\u0061""#,
        r#""\ud834\udd1e""#,
        r#""\ud800""#,
    ];
    // Act / Assert
    for value in values {
        let expected = serde_json::from_str::<String>(value);
        let actual = decode(value, &controls);
        match (actual, expected) {
            (Ok(actual), Ok(expected)) => assert_eq!(actual.get(), &expected),
            (Err(actual), Err(expected)) => assert_eq!(actual.to_string(), expected.to_string()),
            _ => panic!("decoder mismatch for {value}"),
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_keep_the_decoded_key_reserved_until_its_owner_drops() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let value = format!("\"{}\"", "a".repeat(1024));
    // Act
    let decoded = decode(&value, &controls).expect("decoded");
    // Assert
    assert!(controls.current_query_memory_bytes() >= decoded.get().capacity());
    drop(decoded);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_a_partly_decoded_key_on_actual_copy_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = controls(cancel.clone());
    let value = format!("\"{}\"", "a".repeat(1024));
    let mut copied = 0;
    // Act
    let result = decode_observing(&value, &controls, || {
        copied += 1;
        cancel.cancel();
    });
    // Assert
    assert_eq!(copied, 1);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_report_a_reached_key_error_at_its_object_relative_position() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let input = "{\n \"good\": 1,\n \"\\ud800\": 2}";
    let offset = input.find("\"\\ud800\"").expect("key");
    let expected = serde_json::from_str::<
        std::collections::BTreeMap<String, Box<serde_json::value::RawValue>>,
    >(input)
    .expect_err("invalid surrogate")
    .to_string();
    // Act
    let actual = decode_at(
        input,
        RawPosition {
            offset,
            line: 3,
            column: 1,
        },
        &controls,
    )
    .expect_err("invalid key");
    // Assert
    assert_eq!(actual.to_string(), expected);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_count_multibyte_key_prefixes_as_bytes_in_diagnostics() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let input = "{\"é\":1, \"\\ud800\": 2}";
    let offset = input.find("\"\\ud800\"").expect("key");
    let expected = serde_json::from_str::<
        std::collections::BTreeMap<String, Box<serde_json::value::RawValue>>,
    >(input)
    .expect_err("invalid surrogate")
    .to_string();
    // Act
    let actual = decode_at(
        input,
        RawPosition {
            offset,
            line: 1,
            column: offset,
        },
        &controls,
    )
    .expect_err("invalid key");
    // Assert
    assert_eq!(actual.to_string(), expected);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
