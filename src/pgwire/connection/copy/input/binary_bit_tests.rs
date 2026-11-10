use std::time::Instant;

use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn frame(length: i32, payload: &[u8]) -> Vec<u8> {
    let mut frame = length.to_be_bytes().to_vec();
    frame.extend_from_slice(payload);
    frame
}

#[test]
fn should_match_bit_binary_output_with_malformed_error_precedence() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let values = [
        frame(0, &[]),
        frame(1, &[0xff]),
        frame(7, &[0xab]),
        frame(8, &[0xab]),
        frame(9, &[0xab, 0xff]),
        frame(15, &[0x80, 0xff]),
        frame(16, &[0, 0xff]),
    ];
    // Act / Assert
    for oid in [1560, 1562] {
        for bytes in &values {
            let original =
                super::super::super::super::codecs::binary_to_value(bytes, oid).expect("original");
            let expected = super::super::super::super::codecs::value_to_text(&original, oid);
            let (actual, memory) = super::decode(oid, bytes, 0, &controls)
                .expect("decode")
                .expect("handled");
            // Assert
            assert_eq!(actual, expected);
            drop((actual, memory));
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
        for bytes in [
            vec![],
            vec![0, 0, 0],
            frame(-1, &[]),
            frame(i32::MAX, &[]),
            frame(9, &[0xff]),
            frame(1, &[0xff, 0xff]),
        ] {
            let original = super::super::super::super::codecs::binary_to_value(&bytes, oid)
                .expect_err("invalid");
            let result = super::decode(oid, &bytes, 2, &controls);
            assert!(
                matches!(result, Err(CassieError::Parse(message)) if message == format!("COPY binary field 3 is invalid: {original}"))
            );
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }
}

#[test]
fn should_admit_bit_output_at_exact_budget_with_deny_below() {
    // Arrange
    let bytes = frame(9, &[0xaa, 0xff]);
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 15,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(bytes.len()).expect("input");
    // Act
    let (actual, memory) = super::decode(1560, &bytes, 0, &controls)
        .expect("decode")
        .expect("handled");
    // Assert
    assert_eq!(actual, "101010101");
    assert_eq!(controls.current_query_memory_bytes(), 15);
    drop((actual, memory));
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let denied_limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 14,
        ..CassieRuntimeLimits::default()
    };
    let denied = QueryExecutionControls::from_limits(&denied_limits, Instant::now());
    let input = denied.reserve_query_memory(bytes.len()).expect("input");
    assert!(super::decode(1560, &bytes, 0, &denied).is_err());
    assert_eq!(denied.current_query_memory_bytes(), bytes.len());
    drop(input);
    assert_eq!(denied.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_bit_output_when_canceled_during_expansion() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let bytes = frame(5000, &[0xff; 625]);
    // Act
    let result = super::decode_observed(1562, &bytes, 0, &controls, || cancellation.cancel());
    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
