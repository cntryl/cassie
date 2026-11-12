use super::{Cursor, CursorError};
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_match_slice_read_double_error_precedence_and_position() {
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
    // Act / Assert
    for input in inputs {
        let expected = serde_json::from_slice::<String>(input).expect_err("legacy error");
        let mut cursor = Cursor::from_row_bytes(input, &controls);
        let actual = match cursor.read_row_string_observing(|| {}) {
            Ok(_) => panic!("accepted malformed string"),
            Err(error) => error,
        };
        let (category, position) = actual.test_info().expect("semantic error");
        assert_eq!(category, expected.classify(), "{input:?}");
        assert_eq!(
            (position.line, position.column),
            (expected.line(), expected.column()),
            "{input:?}"
        );
        assert_eq!(
            actual.test_message().expect("message"),
            expected.to_string(),
            "{input:?}"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
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
fn should_match_invalid_key_position_after_newlines_and_escapes() {
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
    let actual = match cursor.read_row_string_observing(|| {}) {
        Ok(_) => panic!("accepted invalid key"),
        Err(error) => error,
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
            crate::app::CassieError::QueryCancelled
        ))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_old_and_new_buffers_after_actual_growth_move_cancellation() {
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
            crate::app::CassieError::QueryCancelled
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
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let mut cursor = Cursor::from_row_bytes(input, &controls);
        let result = cursor.read_row_string_observing(|| {});
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
