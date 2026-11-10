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
fn should_match_legacy_string_containment_with_error_positions() {
    // Arrange
    let cases = [
        (r#""a""#, r#""\u0061""#),
        (r#""éclair""#, r#""éclai""#),
        (r#""\ud800""#, r#""\udfff""#),
        (r#""ok""#, r#""\udfff""#),
        (r#"["x","é"]"#, r#""\u00e9""#),
    ];
    let controls = controls(10 * 1024 * 1024);
    // Act / Assert
    for (left, right) in cases {
        let left = ScalarValue::Json(left.into());
        let right = ScalarValue::Json(right.into());
        let legacy = evaluate_with_controls(&contains(), &left, &right, None)
            .map_err(|error| error.to_string());
        let actual = evaluate_with_controls(&contains(), &left, &right, Some(&controls))
            .map_err(|error| error.to_string());
        match (legacy, actual) {
            // Assert
            (Ok(ScalarValue::Bool(a)), Ok(ScalarValue::Bool(b))) => assert_eq!(a, b),
            (Err(a), Err(b)) => assert_eq!(a, b),
            _ => panic!("containment parity"),
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_string_owners_after_actual_copy_with_comparison_cancellation() {
    // Arrange
    for during_compare in [false, true] {
        let cancel = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancel.clone(),
        );
        let value = ScalarValue::Json(format!("\"{}\"", "é".repeat(2_048)));
        if during_compare {
            walk::cancel_after_string_compare_once(cancel);
        } else {
            walk::cancel_after_string_copy_once(cancel);
        }
        // Act
        let result = evaluate_with_controls(&contains(), &value, &value, Some(&controls));
        // Assert
        assert!(result.is_err());
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_admit_the_measured_string_comparison_overlap() {
    // Arrange
    let candidate = ScalarValue::Json(format!("\"{}\"", "abcé".repeat(512)));
    let baseline = controls(10 * 1024 * 1024);
    let result = evaluate_with_controls(&contains(), &candidate, &candidate, Some(&baseline))
        .expect("baseline");
    // Assert
    assert!(matches!(result, ScalarValue::Bool(true)));
    let peak = baseline.peak_query_memory_bytes();
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, expected) in [(peak, true), (peak - 1, false)] {
        let controls = controls(budget);
        let result = evaluate_with_controls(&contains(), &candidate, &candidate, Some(&controls));
        assert_eq!(result.is_ok(), expected);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
