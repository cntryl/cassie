use super::{Cursor, CursorError};
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_match_slice_read_error_for_strings_with_competing_errors() {
    // Arrange
    let controls = controls();
    let inputs: &[&[u8]] = &[
        b"\"\xff\\q\"",
        b"\"\xff\x01\"",
        b"\"\xff",
        b"\"\xff\"",
        b"\"a\\n\xffz\"",
        b"\"\\u00e9\xff\"",
        b"\"\\ud800\xff\"",
        b"\"\\u12x4\xff\"",
    ];

    // Act
    let results: Vec<_> = inputs
        .iter()
        .map(|input| {
            let expected = serde_json::from_slice::<String>(input).expect_err("legacy error");
            let mut cursor = Cursor::from_row_bytes(input, &controls);
            let Err(actual) = cursor.read_row_string_observing(|| {}) else {
                panic!("accepted malformed string");
            };
            let (category, position) = actual.test_info().expect("semantic error");
            (
                *input,
                (category, position.line, position.column),
                (expected.classify(), expected.line(), expected.column()),
                (
                    actual.test_message().expect("message"),
                    expected.to_string(),
                ),
                controls.current_query_memory_bytes(),
            )
        })
        .collect();

    // Assert
    for (input, actual, expected, (actual_message, expected_message), memory) in results {
        assert_eq!(actual, expected, "{input:?}");
        assert_eq!(actual_message, expected_message, "{input:?}");
        assert_eq!(memory, 0);
    }
}

#[test]
fn should_decode_a_multibyte_character_across_validation_chunks() {
    // Arrange
    let controls = controls();
    let expected = format!("{}é", "x".repeat(1023));
    let input = serde_json::to_vec(&expected).expect("JSON");
    let mut cursor = Cursor::from_row_bytes(&input, &controls);
    // Act
    let actual = cursor.read_row_string_observing(|| {}).expect("string");
    // Assert
    assert_eq!(actual.as_str(), expected);
    drop(actual);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_legacy_invalid_key_error_in_multiline_input() {
    // Arrange
    let controls = controls();
    let input = b"{\n \"a\":0,\n \"\xff\\nX\":1}";
    let expected = serde_json::from_slice::<serde_json::Value>(input).expect_err("legacy");
    let key_start = input
        .iter()
        .position(|byte| *byte == 0xff)
        .expect("invalid byte")
        - 1;
    let mut cursor = Cursor::from_row_bytes(input, &controls);
    for _ in 0..key_start {
        let _consumed = cursor.next().expect("advance");
    }
    // Act
    let Err(actual) = cursor.read_row_string_observing(|| {}) else {
        panic!("accepted invalid key");
    };
    // Assert
    let (category, position) = actual.test_info().expect("semantic error");
    assert_eq!(category, expected.classify());
    assert_eq!(
        (position.line, position.column),
        (expected.line(), expected.column())
    );
    assert_eq!(
        actual.test_message().expect("message"),
        expected.to_string()
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_row_string_buffers_after_actual_byte_copy_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let mut cursor = Cursor::from_row_bytes(br#""payload""#, &controls);
    let mut copies = 0;
    // Act
    let result = cursor.read_row_string_observing(|| {
        copies += 1;
        cancel.cancel();
    });
    // Assert
    assert_eq!(copies, 1);
    assert!(matches!(
        result,
        Err(CursorError::Interrupted(
            crate::app::CassieError::QueryCanceled
        ))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_buffers_after_actual_growth_move_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let mut cursor = Cursor::from_row_bytes(br#""abcdefghijklmnop""#, &controls);
    let mut moves = 0;
    // Act
    let result = cursor.read_row_string_observing(|| {
        moves += 1;
        if moves == 9 {
            cancel.cancel();
        }
    });
    // Assert
    assert_eq!(moves, 9);
    assert!(matches!(
        result,
        Err(CursorError::Interrupted(
            crate::app::CassieError::QueryCanceled
        ))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_measured_row_string_buffer_overlap_budget() {
    // Arrange
    let input = "\"abcdefghijklmnop é\"".as_bytes();
    let baseline = controls();
    let mut cursor = Cursor::from_row_bytes(input, &baseline);
    let output = cursor.read_row_string_observing(|| {}).expect("string");
    let peak = baseline.peak_query_memory_bytes();
    drop(output);
    assert_eq!(baseline.current_query_memory_bytes(), 0);

    // Act
    let results: Vec<_> = [(peak, true), (peak - 1, false)]
        .into_iter()
        .map(|(budget, succeeds)| {
            let limits = CassieRuntimeLimits {
                query_memory_budget_bytes: budget,
                query_timeout_ms: 0,
                ..CassieRuntimeLimits::default()
            };
            let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
            let mut cursor = Cursor::from_row_bytes(input, &controls);
            let result = cursor.read_row_string_observing(|| {});
            let accepted = result.is_ok();
            drop(result);
            (succeeds, accepted, controls.current_query_memory_bytes())
        })
        .collect();

    // Assert
    for (succeeds, accepted, memory) in results {
        assert_eq!(accepted, succeeds);
        assert_eq!(memory, 0);
    }
}
