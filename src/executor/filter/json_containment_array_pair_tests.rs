use super::{evaluate_observing, evaluate_with_controls, BinaryOp, ScalarValue, TypedOperator};
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
fn op(reverse: bool) -> BinaryOp {
    BinaryOp::Typed {
        operator: if reverse {
            TypedOperator::ContainedOrEqual
        } else {
            TypedOperator::ContainsOrEqual
        },
    }
}

#[test]
fn should_match_legacy_array_containment_retry_and_root_rules() {
    // Arrange
    let cases = [
        ("[1,2]", "[2]"),
        ("[1]", "[1,1]"),
        ("[1,2]", "3"),
        (r#"[{"a":[1]}]"#, r#"[{"a":1}]"#),
        (r#"["\ud800","a"]"#, r#"["a"]"#),
        ("[1e9999999999,1]", "[1]"),
        ("[1]", "[1e9999999999]"),
        ("[1,", "[2,"),
        ("[1]", "[2,"),
        ("[]", "[]"),
        ("[1,2]", "[2,1]"),
    ];
    let controls = controls(20 * 1024 * 1024);
    // Act / Assert
    for (left, right) in cases {
        for reverse in [false, true] {
            let left = ScalarValue::Json(left.into());
            let right = ScalarValue::Json(right.into());
            let expected = evaluate_with_controls(&op(reverse), &left, &right, None)
                .map_err(|error| error.to_string());
            let actual = evaluate_with_controls(&op(reverse), &left, &right, Some(&controls))
                .map_err(|error| error.to_string());
            match (expected, actual) {
                (Ok(ScalarValue::Bool(a)), Ok(ScalarValue::Bool(b))) => assert_eq!(a, b),
                (Err(a), Err(b)) => assert_eq!(a, b),
                _ => panic!("array containment parity"),
            }
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }
}

#[test]
fn should_preserve_array_scalar_observer_order() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    let left = ScalarValue::Json("[1,2,3]".into());
    let right = ScalarValue::Json("[3,2]".into());
    let (mut expected, mut actual) = (Vec::new(), Vec::new());
    // Act
    let legacy = evaluate_observing(&op(false), &left, &right, None, |value| {
        expected.push(value);
    })
    .expect("legacy");
    let controlled = evaluate_observing(&op(false), &left, &right, Some(&controls), |value| {
        actual.push(value);
    })
    .expect("controlled");
    // Assert
    assert!(matches!((legacy, controlled), (ScalarValue::Bool(a), ScalarValue::Bool(b)) if a == b));
    assert_eq!(actual, expected);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_arrays_after_actual_directory_move_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let candidate = ScalarValue::Json("[1,2,3,4,5,6,7]".into());
    let required = ScalarValue::Json("[1]".into());
    crate::executor::filter::json_delete::cancel_containment_array_move_once(cancel);
    // Act
    let result = evaluate_with_controls(&op(false), &candidate, &required, Some(&controls));
    // Assert
    assert!(result.is_err());
    assert!(controls.peak_query_memory_bytes() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_measured_array_directory_overlap() {
    // Arrange
    let candidate = ScalarValue::Json("[1,2,3,4,5,6,7]".into());
    let required = ScalarValue::Json("[7,1]".into());
    let baseline = controls(10 * 1024 * 1024);
    assert!(matches!(
        evaluate_with_controls(&op(false), &candidate, &required, Some(&baseline)),
        Ok(ScalarValue::Bool(true))
    ));
    let peak = baseline.peak_query_memory_bytes();
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, expected) in [(peak, true), (peak - 1, false)] {
        let controls = controls(budget);
        let result = evaluate_with_controls(&op(false), &candidate, &required, Some(&controls));
        assert_eq!(result.is_ok(), expected);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
