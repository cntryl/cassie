use std::time::Instant;

use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

#[test]
fn should_match_binary_timestamptz_default_utc_and_range_behavior() {
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
    let values = [
        i64::MIN,
        bc,
        ad,
        MIN_TIMESTAMP,
        -86_400_000_001,
        -1,
        0,
        1,
        43_200_000_123,
        END_TIMESTAMP - 1,
        i64::MAX,
    ];
    // Act / Assert
    for value in values {
        let bytes = value.to_be_bytes();
        let original =
            super::super::super::super::codecs::binary_to_value(&bytes, 1184).expect("original");
        let expected = super::super::super::super::codecs::value_to_text(&original, 1184);
        let (actual, memory) = super::decode(1184, &bytes, 0, &controls)
            .expect("decode")
            .expect("handled");
        assert_eq!(actual, expected, "timestamptz {value}");
        drop((actual, memory));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    let invalid = [
        (vec![], "08P01", "insufficient data left in message"),
        (vec![0; 7], "08P01", "insufficient data left in message"),
        (vec![0; 9], "22P03", "incorrect binary data format"),
        (
            (i64::MIN + 1).to_be_bytes().to_vec(),
            "22008",
            "timestamp out of range",
        ),
        (
            (MIN_TIMESTAMP - 1).to_be_bytes().to_vec(),
            "22008",
            "timestamp out of range",
        ),
        (
            END_TIMESTAMP.to_be_bytes().to_vec(),
            "22008",
            "timestamp out of range",
        ),
        (
            (i64::MAX - 1).to_be_bytes().to_vec(),
            "22008",
            "timestamp out of range",
        ),
        (
            [END_TIMESTAMP.to_be_bytes().as_slice(), &[0]].concat(),
            "22008",
            "timestamp out of range",
        ),
    ];
    for (bytes, state, message) in invalid {
        let result = super::decode(1184, &bytes, 2, &controls);
        assert!(
            matches!(result, Err(CassieError::TypeInput(error)) if error.sqlstate == state && error.message == message)
        );
    }
}

#[test]
fn should_admit_binary_timestamptz_output_and_honor_cancellation() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 30,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(8).expect("input");
    // Act
    let (actual, memory) = super::decode(1184, &0_i64.to_be_bytes(), 0, &controls)
        .expect("decode")
        .expect("handled");
    // Assert
    assert_eq!(actual, "2000-01-01 00:00:00+00");
    assert_eq!(controls.current_query_memory_bytes(), 30);
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
        super::decode(1184, &0_i64.to_be_bytes(), 0, &cancelled),
        Err(CassieError::QueryCancelled)
    ));
}

#[test]
fn should_deny_binary_timestamptz_below_output_overlap_budget() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 29,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(8).expect("input");
    // Act
    let result = super::decode(1184, &0_i64.to_be_bytes(), 0, &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 8);
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
