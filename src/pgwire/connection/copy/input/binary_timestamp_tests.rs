use std::time::Instant;

use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

#[test]
fn should_match_binary_timestamp_output_and_error_precedence() {
    // Arrange
    use crate::types::extended::temporal::calendar::{
        fields_to_timestamp, Fields, END_TIMESTAMP, MIN_TIMESTAMP,
    };
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let bc = fields_to_timestamp(
        &Fields {
            year: 0,
            month: 12,
            day: 31,
            hour: 23,
            minute: 59,
            second: 59,
            micros: 999_999,
        },
        None,
    )
    .expect("BC");
    let ad = fields_to_timestamp(
        &Fields {
            year: 1,
            month: 1,
            day: 1,
            ..Fields::default()
        },
        None,
    )
    .expect("AD");
    let timestamps = [
        i64::MIN,
        bc,
        ad,
        MIN_TIMESTAMP,
        MIN_TIMESTAMP + 1,
        -86_400_000_001,
        -1,
        0,
        1,
        1_000_000,
        43_200_000_123,
        END_TIMESTAMP - 1,
        i64::MAX,
    ];
    // Act / Assert
    for timestamp in timestamps {
        let bytes = timestamp.to_be_bytes();
        let original =
            super::super::super::super::codecs::binary_to_value(&bytes, 1114).expect("original");
        let expected = super::super::super::super::codecs::value_to_text(&original, 1114);
        let (actual, memory) = super::decode(1114, &bytes, 0, &controls)
            .expect("decode")
            .expect("handled");
        assert_eq!(actual, expected, "timestamp {timestamp}");
        drop((actual, memory));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    for bytes in [
        vec![],
        vec![0; 7],
        vec![0; 9],
        (MIN_TIMESTAMP - 1).to_be_bytes().to_vec(),
        END_TIMESTAMP.to_be_bytes().to_vec(),
        [(MIN_TIMESTAMP - 1).to_be_bytes().as_slice(), &[0]].concat(),
    ] {
        let original =
            super::super::super::super::codecs::binary_to_value(&bytes, 1114).expect_err("invalid");
        let result = super::decode(1114, &bytes, 2, &controls);
        assert!(
            matches!(result, Err(CassieError::Parse(message)) if message == format!("COPY binary field 3 is invalid: {original}"))
        );
    }
}

#[test]
fn should_admit_binary_timestamp_output_and_honor_cancellation() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 27,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(8).expect("input");
    // Act
    let (actual, memory) = super::decode(1114, &0_i64.to_be_bytes(), 0, &controls)
        .expect("decode")
        .expect("handled");
    // Assert
    assert_eq!(actual, "2000-01-01 00:00:00");
    assert_eq!(controls.current_query_memory_bytes(), 27);
    drop((actual, memory));
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let cancellation = QueryCancellationHandle::new();
    let cancelled = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();
    assert!(matches!(
        super::decode(1114, &0_i64.to_be_bytes(), 0, &cancelled),
        Err(CassieError::QueryCancelled)
    ));
}

#[test]
fn should_deny_binary_timestamp_below_output_overlap_budget() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 26,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(8).expect("input");
    // Act
    let result = super::decode(1114, &0_i64.to_be_bytes(), 0, &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 8);
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
