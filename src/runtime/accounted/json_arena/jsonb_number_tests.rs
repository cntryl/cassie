use std::time::Instant;

use super::{format_number, format_number_observed, FormatError, Progress};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_match_numeric_parse_and_display_for_valid_json_numbers() {
    // Arrange
    let controls = controls();
    let inputs = [
        "0",
        "-0",
        "0.00",
        "-0.000e+9",
        "1",
        "-1.2300",
        "1e+2",
        "1e-2",
        "1e+131071",
        "1e-16383",
        "12.30e-1",
        "12.30e+1",
        "12.30e+2",
        "12.30e+3",
        "-0.000e-0",
        "3.00e-00002",
        "1000.000e-2",
        "0.00012300",
        "123456789012345678901234567890.125",
        "0e+1073741823",
        "1e+0000000000000000000002",
    ];

    // Act / Assert
    for input in inputs {
        let expected = crate::types::extended::numeric::parse_text(input)
            .expect("numeric")
            .to_string();
        let actual = format_number(input, &controls).expect("formatted");
        assert_eq!(actual.as_str(), expected, "{input}");
        drop(actual);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_match_numeric_overflow_state_and_message() {
    // Arrange
    let controls = controls();
    let inputs = [
        "1e-16384",
        "0e-16384",
        "1e+131072",
        "0e+1073741824",
        "-0e-999999999999999999999999",
    ];

    // Act / Assert
    for input in inputs {
        let expected = crate::types::extended::numeric::parse_text(input).expect_err("overflow");
        assert_eq!(
            (expected.sqlstate, expected.message.as_str()),
            ("22003", "value overflows numeric format")
        );
        assert!(
            matches!(format_number(input, &controls), Err(FormatError::Overflow)),
            "{input}"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_admit_exact_decimal_output_capacity_and_deny_below() {
    // Arrange
    let exact = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 4,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let below = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 3,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );

    // Act / Assert
    let output = format_number("1.25", &exact).expect("exact");
    assert_eq!(output.as_str(), "1.25");
    assert!(matches!(
        format_number("1.25", &below),
        Err(FormatError::Control(CassieError::ResourceLimit(_)))
    ));
    assert_eq!(below.current_query_memory_bytes(), 0);
    drop(output);
    assert_eq!(exact.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_during_actual_decimal_scan_and_release_owners() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let input = format!("1.{}", "0".repeat(2048));
    let mut scanned = false;

    // Act
    let result = format_number_observed(&input, &controls, |progress| {
        if matches!(progress, Progress::Scan) {
            scanned = true;
            cancellation.cancel();
        }
    });

    // Assert
    assert!(scanned);
    assert!(matches!(
        result,
        Err(FormatError::Control(CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_during_actual_decimal_output_and_release_owners() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut emitted = false;

    // Act
    let result = format_number_observed("0e-1500", &controls, |progress| {
        if matches!(progress, Progress::Output) {
            emitted = true;
            cancellation.cancel();
        }
    });

    // Assert
    assert!(emitted);
    assert!(matches!(
        result,
        Err(FormatError::Control(CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
