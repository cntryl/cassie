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
fn should_match_legacy_root_objects_including_lexical_order_and_nested_rules() {
    // Arrange
    let cases = [
        (r#"{"z":1e9999999999}"#, r#"{"z":1e9999999999,"aa":1}"#),
        (r#"{"a":1,"\u0061":2}"#, r#"{"a":2}"#),
        (r#"{"a":[1]}"#, r#"{"a":1}"#),
        (r#"{"a":[1,2]}"#, r#"{"a":[2]}"#),
        (r#"{"\ud800":1}"#, r#"{"\udfff":2}"#),
        (r#"{"a":1}"#, r#"{"\udfff":2}"#),
        ("\n  {\"\\ud800\":1}", "{}"),
        ("{}", "\n  {\"\\udfff\":1}"),
        (r#"{"a":1}"#, r#"{"a":1,"z":2}"#),
        ("{]", "{]"),
        (r#"{"a":1}"#, "{]"),
        (r#"{"é":1,"aa":2}"#, r#"{"é":1}"#),
    ];
    let controls = controls(10 * 1024 * 1024);
    // Act / Assert
    for (left, right) in cases {
        for reverse in [false, true] {
            let left = ScalarValue::Json(left.into());
            let right = ScalarValue::Json(right.into());
            let legacy = evaluate_with_controls(&op(reverse), &left, &right, None)
                .map_err(|error| error.to_string());
            let actual = evaluate_with_controls(&op(reverse), &left, &right, Some(&controls))
                .map_err(|error| error.to_string());
            match (legacy, actual) {
                (Ok(ScalarValue::Bool(a)), Ok(ScalarValue::Bool(b))) => assert_eq!(a, b),
                (Err(a), Err(b)) => assert_eq!(a, b),
                _ => panic!("containment parity"),
            }
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }
}

#[test]
fn should_preserve_scalar_observer_through_root_object_children() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    let left = ScalarValue::Json(r#"{"a":1,"b":[2,3]}"#.into());
    let right = ScalarValue::Json(r#"{"a":1,"b":[3]}"#.into());
    let mut legacy = Vec::new();
    let mut actual = Vec::new();
    // Act
    let expected = evaluate_observing(&op(false), &left, &right, None, |value| legacy.push(value))
        .expect("legacy");
    let result = evaluate_observing(&op(false), &left, &right, Some(&controls), |value| {
        actual.push(value);
    })
    .expect("controlled");
    // Assert
    assert!(matches!((expected, result), (ScalarValue::Bool(a), ScalarValue::Bool(b)) if a == b));
    assert_eq!(actual, legacy);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_root_object_after_actual_sort_and_merge_cancellation() {
    // Arrange
    for sorting in [false, true] {
        let cancel = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancel.clone(),
        );
        let left = ScalarValue::Json(r#"{"z":1,"aa":2}"#.into());
        let right = ScalarValue::Json(r#"{"z":1,"aa":2}"#.into());
        if sorting {
            crate::executor::filter::json_delete::cancel_containment_key_sort_once(cancel);
        } else {
            crate::executor::filter::json_delete::cancel_containment_key_compare_once(cancel);
        }
        // Act
        let result = evaluate_with_controls(&op(false), &left, &right, Some(&controls));
        // Assert
        assert!(result.is_err());
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_admit_measured_root_object_overlap() {
    // Arrange
    let left = ScalarValue::Json(r#"{"z":1,"aa":2}"#.into());
    let right = ScalarValue::Json(r#"{"z":1,"aa":2}"#.into());
    let baseline = controls(10 * 1024 * 1024);
    assert!(matches!(
        evaluate_with_controls(&op(false), &left, &right, Some(&baseline)),
        Ok(ScalarValue::Bool(true))
    ));
    let peak = baseline.peak_query_memory_bytes();
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, expected) in [(peak, true), (peak - 1, false)] {
        let controls = controls(budget);
        let result = evaluate_with_controls(&op(false), &left, &right, Some(&controls));
        assert_eq!(result.is_ok(), expected);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
