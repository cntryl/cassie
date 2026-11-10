use std::time::Instant;

use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn frame(micros: i64, days: i32, months: i32) -> Vec<u8> {
    [
        micros.to_be_bytes().as_slice(),
        days.to_be_bytes().as_slice(),
        months.to_be_bytes().as_slice(),
    ]
    .concat()
}

#[test]
fn should_match_binary_interval_default_style_with_error_precedence() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let values = [
        (0, 0, 0),
        (1, 0, 0),
        (-1, 0, 0),
        (1_234_567, -1, 13),
        (-1_234_567, 1, -13),
        (0, -1, 12),
        (0, 1, -12),
        (i64::MIN, i32::MIN, i32::MIN),
        (i64::MAX, i32::MAX, i32::MAX),
        (i64::MIN, i32::MAX, i32::MIN),
        (i64::MAX, i32::MIN, i32::MAX),
        (i64::MIN + 1, i32::MIN, i32::MIN),
        (i64::MAX - 1, i32::MAX, i32::MAX),
    ];
    // Act / Assert
    for (micros, days, months) in values {
        let bytes = frame(micros, days, months);
        let original =
            super::super::super::super::codecs::binary_to_value(&bytes, 1186).expect("original");
        let expected = super::super::super::super::codecs::value_to_text(&original, 1186);
        let (actual, memory) = super::decode(1186, &bytes, 0, &controls)
            .expect("decode")
            .expect("handled");
        // Assert
        assert_eq!(
            actual, expected,
            "micros {micros} days {days} months {months}"
        );
        drop((actual, memory));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    let invalid = [
        (vec![], "08P01", "insufficient data left in message"),
        (vec![0; 15], "08P01", "insufficient data left in message"),
        (vec![0; 17], "22P03", "incorrect binary data format"),
    ];
    for (bytes, state, message) in invalid {
        let result = super::decode(1186, &bytes, 2, &controls);
        assert!(
            matches!(result, Err(CassieError::TypeInput(error)) if error.sqlstate == state && error.message == message)
        );
    }
}

#[test]
fn should_enforce_binary_interval_overlap_budget_with_cancellation() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 24,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(16).expect("input");
    // Act
    let (actual, memory) = super::decode(1186, &frame(0, 0, 0), 0, &controls)
        .expect("decode")
        .expect("handled");
    // Assert
    assert_eq!(actual, "00:00:00");
    assert_eq!(controls.current_query_memory_bytes(), 24);
    drop((actual, memory));
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 23,
        ..CassieRuntimeLimits::default()
    };
    let denied = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = denied.reserve_query_memory(16).expect("input");
    assert!(super::decode(1186, &frame(0, 0, 0), 0, &denied).is_err());
    assert_eq!(denied.current_query_memory_bytes(), 16);
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
        super::decode(1186, &frame(0, 0, 0), 0, &cancelled),
        Err(CassieError::QueryCancelled)
    ));
}
