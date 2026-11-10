use super::{evaluate_with_controls, BinaryOp, ScalarValue, TypedOperator};
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
fn should_match_legacy_nested_object_order_errors_with_duplicates() {
    // Arrange
    let cases = [
        (r#"[{"z":1e9999999999}]"#, r#"[{"z":1e9999999999,"aa":1}]"#),
        (r#"[{"a":1,"\u0061":2}]"#, r#"[{"a":2}]"#),
        (r#"[{"a":[1]}]"#, r#"[{"a":1}]"#),
        (r#"[{"\ud800":1}]"#, r#"[{"\udfff":2}]"#),
        (r#"[{"a":1}]"#, r#"[{"\udfff":2}]"#),
        (r#"[{"outer":{"k":1}}]"#, r#"[{"outer":{"k":1}}]"#),
    ];
    let controls = controls(20 * 1024 * 1024);
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
                // Assert
                (Ok(ScalarValue::Bool(a)), Ok(ScalarValue::Bool(b))) => assert_eq!(a, b),
                (Err(a), Err(b)) => assert_eq!(a, b),
                _ => panic!("containment parity"),
            }
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }
}

#[test]
fn should_compare_deep_nested_objects_without_recursive_frames() {
    // Arrange
    let controls = controls(100 * 1024 * 1024);
    let mut text = "1".to_string();
    for _ in 0..160 {
        text = format!("{{\"k\":{text}}}");
    }
    let value = ScalarValue::Json(format!("[{text}]"));
    // Act
    let result = evaluate_with_controls(&op(false), &value, &value, Some(&controls));
    // Assert
    assert!(matches!(result, Ok(ScalarValue::Bool(true))));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_nested_sources_after_actual_index_work_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let left = ScalarValue::Json(r#"[{"z":1,"aa":2}]"#.into());
    let right = ScalarValue::Json(r#"[{"z":1,"aa":2}]"#.into());
    crate::executor::filter::json_delete::cancel_nested_object_index_once(cancellation);
    // Act
    let result = evaluate_with_controls(&op(false), &left, &right, Some(&controls));
    // Assert
    assert!(result.is_err());
    assert!(controls.peak_query_memory_bytes() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_measured_nested_object_overlap() {
    // Arrange
    let value = ScalarValue::Json(r#"[{"z":1,"aa":2}]"#.into());
    let baseline = controls(10 * 1024 * 1024);
    // Assert
    assert!(matches!(
        evaluate_with_controls(&op(false), &value, &value, Some(&baseline)),
        Ok(ScalarValue::Bool(true))
    ));
    let peak = baseline.peak_query_memory_bytes();
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, expected) in [(peak, true), (peak - 1, false)] {
        let controls = controls(budget);
        let result = evaluate_with_controls(&op(false), &value, &value, Some(&controls));
        assert_eq!(result.is_ok(), expected);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
