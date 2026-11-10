use std::time::Instant;

use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

#[test]
fn should_match_binary_date_output_with_error_precedence() {
    // Arrange
    use crate::types::extended::temporal::calendar::{
        date2j, DATE_END_JULIAN, POSTGRES_EPOCH_JDATE,
    };
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let first = -POSTGRES_EPOCH_JDATE;
    let last = i32::try_from(DATE_END_JULIAN - i64::from(POSTGRES_EPOCH_JDATE) - 1).expect("last");
    let bc = i32::try_from(date2j(0, 12, 31) - i64::from(POSTGRES_EPOCH_JDATE)).expect("BC");
    let ad = i32::try_from(date2j(1, 1, 1) - i64::from(POSTGRES_EPOCH_JDATE)).expect("AD");
    let dates = [i32::MIN, first, first + 1, bc, ad, -1, 0, 1, last, i32::MAX];
    // Act / Assert
    for date in dates {
        let bytes = date.to_be_bytes();
        let original =
            super::super::super::super::codecs::binary_to_value(&bytes, 1082).expect("original");
        let expected = super::super::super::super::codecs::value_to_text(&original, 1082);
        let (actual, memory) = super::decode(1082, &bytes, 0, &controls)
            .expect("decode")
            .expect("handled");
        // Assert
        assert_eq!(actual, expected, "date {date}");
        drop((actual, memory));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    let invalid = [
        (vec![], "08P01", "insufficient data left in message"),
        (vec![0; 3], "08P01", "insufficient data left in message"),
        (vec![0; 5], "22P03", "incorrect binary data format"),
        (
            (first - 1).to_be_bytes().to_vec(),
            "22008",
            "date out of range",
        ),
        (
            (last + 1).to_be_bytes().to_vec(),
            "22008",
            "date out of range",
        ),
        (
            [(first - 1).to_be_bytes().as_slice(), &[0]].concat(),
            "22008",
            "date out of range",
        ),
    ];
    for (bytes, state, message) in invalid {
        let result = super::decode(1082, &bytes, 2, &controls);
        assert!(
            matches!(result, Err(CassieError::TypeInput(error)) if error.sqlstate == state && error.message == message)
        );
    }
}

#[test]
fn should_admit_binary_date_output_with_honor_cancellation() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 14,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(4).expect("input");
    // Act
    let (actual, memory) = super::decode(1082, &0_i32.to_be_bytes(), 0, &controls)
        .expect("decode")
        .expect("handled");
    // Assert
    assert_eq!(actual, "2000-01-01");
    assert_eq!(controls.current_query_memory_bytes(), 14);
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
        super::decode(1082, &0_i32.to_be_bytes(), 0, &cancelled),
        Err(CassieError::QueryCancelled)
    ));
}

#[test]
fn should_deny_binary_date_below_output_overlap_budget() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 13,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(4).expect("input");
    // Act
    let result = super::decode(1082, &0_i32.to_be_bytes(), 0, &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 4);
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
