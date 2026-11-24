use std::time::Instant;

use super::{Cursor, CursorError, Position};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_track_consumed_with_peek_positions_as_utf8_bytes() {
    // Arrange
    let controls = controls();
    let mut cursor = Cursor::from_str("a\né", &controls);

    // Act / Assert
    // Assert
    assert_eq!(
        cursor.position(),
        Position {
            offset: 0,
            line: 1,
            column: 0
        }
    );
    assert_eq!(
        cursor.peek_position().expect("peek"),
        Position {
            offset: 1,
            line: 1,
            column: 1
        }
    );
    assert_eq!(cursor.next().expect("next"), Some(b'a'));
    assert_eq!(
        cursor.peek_position().expect("peek"),
        Position {
            offset: 2,
            line: 2,
            column: 0
        }
    );
    assert_eq!(cursor.next().expect("newline"), Some(b'\n'));
    assert_eq!(cursor.next().expect("first byte"), Some(0xc3));
    assert_eq!(cursor.next().expect("second byte"), Some(0xa9));
    assert_eq!(
        cursor.position(),
        Position {
            offset: 4,
            line: 2,
            column: 2
        }
    );
    assert_eq!(cursor.next().expect("eof"), None);
    assert_eq!(cursor.peek_position().expect("eof peek"), cursor.position());
}

#[test]
fn should_skip_only_json_whitespace() {
    // Arrange
    let controls = controls();
    let mut cursor = Cursor::from_str("\r\n\t x\u{a0}", &controls);

    // Act
    let next = cursor.skip_whitespace().expect("skip");

    // Assert
    assert_eq!(next, Some(b'x'));
    assert_eq!(
        cursor.position(),
        Position {
            offset: 4,
            line: 2,
            column: 2
        }
    );
    assert_eq!(cursor.next().expect("x"), Some(b'x'));
    assert_eq!(cursor.skip_whitespace().expect("nbsp"), Some(0xc2));
}

#[test]
fn should_validate_full_binary_utf8_before_exposing_cursor() {
    // Arrange
    let controls = controls();
    let invalid_syntax_then_invalid_utf8 = b"\x7bx\xff";

    // Act / Assert
    // Assert
    assert!(matches!(
        Cursor::from_bytes(invalid_syntax_then_invalid_utf8, &controls),
        Err(CursorError::InvalidUtf8)
    ));
    assert!(matches!(
        Cursor::from_bytes(b"\xf0\x9f\x99", &controls),
        Err(CursorError::InvalidUtf8)
    ));
}

#[test]
fn should_validate_utf8_that_crosses_a_validation_chunk_boundary() {
    // Arrange
    let controls = controls();
    let mut bytes = vec![b'x'; 1023];
    bytes.extend_from_slice("🙂é".as_bytes());

    // Act
    let cursor = Cursor::from_bytes(&bytes, &controls).expect("valid utf8");

    // Assert
    assert_eq!(
        cursor.position(),
        Position {
            offset: 0,
            line: 1,
            column: 0
        }
    );
    assert_eq!(cursor.peek(), Some(b'x'));
}

#[test]
fn should_validate_each_multibyte_split_with_reject_invalid_continuations() {
    // Arrange
    let controls = controls();

    // Act / Assert
    for scalar in ["é", "€", "🙂"] {
        for split in 1..scalar.len() {
            let mut bytes = vec![b'x'; 1024 - split];
            bytes.extend_from_slice(scalar.as_bytes());
            // Assert
            assert!(
                Cursor::from_bytes(&bytes, &controls).is_ok(),
                "{scalar} split {split}"
            );
            let final_index = bytes.len() - 1;
            bytes[final_index] = b'x';
            assert!(matches!(
                Cursor::from_bytes(&bytes, &controls),
                Err(CursorError::InvalidUtf8)
            ));
            bytes.truncate(final_index);
            assert!(matches!(
                Cursor::from_bytes(&bytes, &controls),
                Err(CursorError::InvalidUtf8)
            ));
        }
    }
}

#[test]
fn should_cap_peek_position_at_empty_with_newline_end_of_input() {
    // Arrange
    let controls = controls();
    let empty = Cursor::from_str("", &controls);
    let mut newline = Cursor::from_str("\n", &controls);

    // Act / Assert
    // Assert
    assert_eq!(
        empty.peek_position().expect("empty"),
        Position {
            offset: 0,
            line: 1,
            column: 0
        }
    );
    assert_eq!(
        newline.peek_position().expect("newline"),
        Position {
            offset: 1,
            line: 2,
            column: 0
        }
    );
    assert_eq!(newline.next().expect("next"), Some(b'\n'));
    assert_eq!(
        newline.peek_position().expect("eof"),
        Position {
            offset: 1,
            line: 2,
            column: 0
        }
    );
}

#[test]
fn should_cancel_after_a_real_binary_utf8_validation_chunk() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let bytes = vec![b'x'; 1025];

    // Act
    let result = Cursor::from_bytes_observed(&bytes, &controls, || cancellation.cancel());

    // Assert
    assert!(matches!(
        result,
        Err(CursorError::Interrupted(CassieError::QueryCanceled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
