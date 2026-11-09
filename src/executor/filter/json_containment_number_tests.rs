use super::{evaluate_with_controls, walk, BinaryOp, ScalarValue, TypedOperator};
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}
fn contains() -> BinaryOp {
    BinaryOp::Typed {
        operator: TypedOperator::ContainsOrEqual,
    }
}

#[test]
fn should_match_legacy_numeric_equality_range_and_error_order() {
    // Arrange
    let cases = [
        ("1", "1.000"),
        ("-0.000", "0e4"),
        ("100e-2", "1"),
        ("-1.20", "-1.2"),
        ("1e2", "100"),
        ("0.00001", "1e-5"),
        ("1.0001", "1.001"),
        ("1e131071", "1e131071"),
        ("1e131072", "1e9999999999"),
        ("1", "1e9999999999"),
        ("0e-16384", "1"),
        ("0e-16383", "0"),
    ];
    let controls = controls(256 * 1024 * 1024);
    // Act / Assert
    for (left, right) in cases {
        let left = ScalarValue::Json(left.into());
        let right = ScalarValue::Json(right.into());
        let legacy = evaluate_with_controls(&contains(), &left, &right, None)
            .map_err(|error| error.to_string());
        let actual = evaluate_with_controls(&contains(), &left, &right, Some(&controls))
            .map_err(|error| error.to_string());
        match (legacy, actual) {
            (Ok(ScalarValue::Bool(a)), Ok(ScalarValue::Bool(b))) => assert_eq!(a, b),
            (Err(a), Err(b)) => assert_eq!(a, b),
            _ => panic!("numeric containment parity"),
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_cancel_after_actual_numeric_format_and_equality_work() {
    // Arrange
    for during_compare in [false, true] {
        let cancel = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancel.clone(),
        );
        let value = ScalarValue::Json(format!("1{}", "2".repeat(2_048)));
        if during_compare {
            walk::cancel_after_number_compare_once(cancel);
        } else {
            walk::cancel_after_number_format_once(cancel);
        }
        // Act
        let result = evaluate_with_controls(&contains(), &value, &value, Some(&controls));
        // Assert
        assert!(result.is_err());
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_admit_measured_numeric_comparison_overlap() {
    // Arrange
    let value = ScalarValue::Json("123456.0000000000000001".into());
    let baseline = controls(10 * 1024 * 1024);
    assert!(matches!(
        evaluate_with_controls(&contains(), &value, &value, Some(&baseline)),
        Ok(ScalarValue::Bool(true))
    ));
    let peak = baseline.peak_query_memory_bytes();
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, expected) in [(peak, true), (peak - 1, false)] {
        let controls = controls(budget);
        let result = evaluate_with_controls(&contains(), &value, &value, Some(&controls));
        assert_eq!(result.is_ok(), expected);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
