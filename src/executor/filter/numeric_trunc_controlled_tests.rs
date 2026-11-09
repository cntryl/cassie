//! Test-first cases for the proposed controlled NUMERIC truncation kernel.
use super::{truncate, truncate_observing, NumericPhase};
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::types::extended::numeric::{math, parse_text};
use std::time::Instant;

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

#[test]
fn should_match_existing_numeric_truncation_across_decimal_chunks() {
    // Arrange
    let cases = [
        ("12345678901234567890.1234567891", 12),
        ("12345678901234567890.1234567891", 1),
        ("-12345678901234567890.1234567891", -10),
        ("0.00000000000000000001", 0),
        ("0", i64::MIN),
        ("0", i64::MAX),
        ("NaN", 2),
        ("Infinity", -2),
        ("-Infinity", 2),
    ];
    let controls = controls(1_000_000);

    // Act and assert
    for (input, scale) in cases {
        let value = parse_text(input).expect("numeric input");
        let expected = math::trunc(&value, scale);
        let output = truncate(&value, scale, &controls).expect("controlled truncation");
        assert_eq!(output.as_numeric(), &expected, "{input} at {scale}");
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_workspace_after_cancellation_during_actual_limb_work() {
    // Arrange
    for (input, scale, target) in [
        ("12345678901234567890.123456789", 0, NumericPhase::Copy),
        ("12345678901234567890.123456789", 0, NumericPhase::Divide),
        ("12345678901234567890", 12, NumericPhase::Multiply),
    ] {
        let value = parse_text(input).expect("numeric input");
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        let mut reached = false;

        // Act
        let result = truncate_observing(&value, scale, &controls, |phase| {
            if phase == target {
                reached = true;
                cancellation.cancel();
            }
        });

        // Assert
        assert!(reached, "did not reach {target:?}");
        assert!(result.is_err(), "cancellation must interrupt {target:?}");
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
