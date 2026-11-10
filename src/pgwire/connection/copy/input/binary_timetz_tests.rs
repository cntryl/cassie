use std::time::Instant;

use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn frame(time: i64, zone: i32) -> Vec<u8> {
    [time.to_be_bytes().as_slice(), zone.to_be_bytes().as_slice()].concat()
}

#[test]
fn should_match_binary_timetz_output_with_error_precedence() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let values = [
        (0, 0),
        (86_400_000_000, 0),
        (43_200_000_123, -7_200),
        (1, -57_599),
        (10, 57_599),
        (100, -60),
        (1_000, 61),
        (100_000, -3_600),
    ];
    // Act / Assert
    for (time, zone) in values {
        let bytes = frame(time, zone);
        let original =
            super::super::super::super::codecs::binary_to_value(&bytes, 1266).expect("original");
        let expected = super::super::super::super::codecs::value_to_text(&original, 1266);
        let (actual, memory) = super::decode(1266, &bytes, 0, &controls)
            .expect("decode")
            .expect("handled");
        // Assert
        assert_eq!(actual, expected, "time {time} zone {zone}");
        drop((actual, memory));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    for (bytes, state) in [
        (frame(-1, i32::MIN), "22008"),
        (frame(0, 57_600), "22009"),
        (vec![0; 11], "08P01"),
        (vec![0; 13], "22P03"),
    ] {
        let result = super::decode(1266, &bytes, 0, &controls);
        assert!(matches!(result, Err(CassieError::TypeInput(error)) if error.sqlstate == state));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_enforce_binary_timetz_overlap_budget_with_cancellation() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 23,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(12).expect("input");
    // Act
    let (actual, memory) = super::decode(1266, &frame(0, 0), 0, &controls)
        .expect("decode")
        .expect("handled");
    // Assert
    assert_eq!(actual, "00:00:00+00");
    assert_eq!(controls.current_query_memory_bytes(), 23);
    drop((actual, memory));
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 22,
        ..CassieRuntimeLimits::default()
    };
    let denied = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = denied.reserve_query_memory(12).expect("input");
    assert!(super::decode(1266, &frame(0, 0), 0, &denied).is_err());
    assert_eq!(denied.current_query_memory_bytes(), 12);
    drop(input);
    assert_eq!(denied.current_query_memory_bytes(), 0);
    let cancellation = QueryCancellationHandle::new();
    let cancelled = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();
    assert!(matches!(
        super::decode(1266, &frame(0, 0), 0, &cancelled),
        Err(CassieError::QueryCancelled)
    ));
}
