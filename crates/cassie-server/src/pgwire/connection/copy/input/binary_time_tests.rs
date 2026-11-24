use std::time::Instant;

use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

#[test]
fn should_match_binary_time_output_with_error_precedence() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let times = [
        0_i64,
        1,
        10,
        100,
        1_000,
        100_000,
        1_000_000,
        3_600_000_000,
        43_200_000_123,
        86_399_999_999,
        86_400_000_000,
    ];
    // Act / Assert
    for time in times {
        let bytes = time.to_be_bytes();
        let original =
            super::super::super::super::codecs::binary_to_value(&bytes, 1083).expect("original");
        let expected = super::super::super::super::codecs::value_to_text(&original, 1083);
        let (actual, memory) = super::decode(1083, &bytes, 0, &controls)
            .expect("decode")
            .expect("handled");
        // Assert
        assert_eq!(actual, expected, "micros {time}");
        drop((actual, memory));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    let invalid = [
        (vec![], "08P01", "insufficient data left in message"),
        (vec![0; 7], "08P01", "insufficient data left in message"),
        (vec![0; 9], "22P03", "incorrect binary data format"),
        (
            (-1_i64).to_be_bytes().to_vec(),
            "22008",
            "time out of range",
        ),
        (
            86_400_000_001_i64.to_be_bytes().to_vec(),
            "22008",
            "time out of range",
        ),
        (
            [(-1_i64).to_be_bytes().as_slice(), &[0]].concat(),
            "22008",
            "time out of range",
        ),
    ];
    for (bytes, state, message) in invalid {
        let result = super::decode(1083, &bytes, 2, &controls);
        assert!(
            matches!(result, Err(CassieError::TypeInput(error)) if error.sqlstate == state && error.message == message)
        );
    }
}

#[test]
fn should_admit_binary_time_output_with_honor_cancellation() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 16,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(8).expect("input");
    // Act
    let (actual, memory) = super::decode(1083, &0_i64.to_be_bytes(), 0, &controls)
        .expect("decode")
        .expect("handled");
    // Assert
    assert_eq!(actual, "00:00:00");
    assert_eq!(controls.current_query_memory_bytes(), 16);
    drop((actual, memory));
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let cancellation = QueryCancellationHandle::new();
    let canceled = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();
    assert!(matches!(
        super::decode(1083, &0_i64.to_be_bytes(), 0, &canceled),
        Err(CassieError::QueryCanceled)
    ));
}

#[test]
fn should_deny_binary_time_below_output_overlap_budget() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 15,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(8).expect("input");
    // Act
    let result = super::decode(1083, &0_i64.to_be_bytes(), 0, &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 8);
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
