use super::{compare, compare_controlled, evaluate_controlled};
use crate::config::CassieRuntimeLimits;
use crate::executor::filter::ScalarValue;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::sql::ast::RowComparisonOp;
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

#[test]
fn should_match_legacy_jsonb_order_for_ranks_objects_numbers_with_sentinels() {
    // Arrange
    let cases = [
        ("[]", "null"),
        ("[[]]", "[null]"),
        ("null", "\"x\""),
        ("1", "true"),
        ("[1]", "[0,0]"),
        (r#"{"z":0}"#, r#"{"aa":0}"#),
        (r#"{"a":1,"a":2}"#, r#"{"a":2}"#),
        ("-0.00", "0"),
        ("-1.20", "-1.19"),
        ("-0.001", "-0.01"),
        ("0.0001000", "0.0001"),
        ("999.1", "1000"),
        ("[1e9999999999]", "[true]"),
        ("[1e9999999999]", "[1]"),
        ("null", "[\u{7d}"),
        ("100e-2", "1"),
        ("18446744073709551615", "18446744073709551616"),
        (r#"{"$serde_json::private::Number":"7"}"#, "7"),
        (
            r#"{"$serde_json::private::RawValue":"{\"x\":1}"}"#,
            r#"{"x":1}"#,
        ),
        (r#"{"a":1e9999999999}"#, r#"{"b":0}"#),
        ("1e131072", "1"),
        ("[\u{7d}", "{}"),
    ];
    let controls = controls(100 * 1024 * 1024);
    // Act
    for (left, right) in cases {
        // Assert
        assert_eq!(
            compare_controlled(left, right, &controls).expect("controlled"),
            compare(left, right),
            "{left} versus {right}"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_null_with_invalid_input_comparison_behavior() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    // Act / Assert
    // Assert
    assert!(matches!(
        evaluate_controlled(
            &ScalarValue::Null,
            RowComparisonOp::Eq,
            &ScalarValue::Json("1".into()),
            &controls
        ),
        Ok(ScalarValue::Null)
    ));
    assert!(matches!(
        evaluate_controlled(
            &ScalarValue::Null,
            RowComparisonOp::IsDistinctFrom,
            &ScalarValue::Json("1".into()),
            &controls
        ),
        Ok(ScalarValue::Bool(true))
    ));
    assert!(evaluate_controlled(
        &ScalarValue::Json("[\u{7d}".into()),
        RowComparisonOp::Eq,
        &ScalarValue::Json("1".into()),
        &controls
    )
    .is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_arena_after_actual_comparison_with_frame_move_cancellation() {
    // Arrange
    for frames in [false, true] {
        let cancel = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancel.clone(),
        );
        let mut value = format!("\"{}\"", "x".repeat(2_048));
        if frames {
            for _ in 0..12 {
                value = format!("[{value}]");
            }
            crate::runtime::accounted::json_arena::cancel_comparison_frame_move_once(cancel);
        } else {
            crate::runtime::accounted::json_arena::cancel_comparison_work_once(cancel);
        }
        // Act
        let result = compare_controlled(&value, &value, &controls);
        // Assert
        assert!(result.is_err());
        assert!(controls.peak_query_memory_bytes() > 0);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_admit_exact_comparison_peak_with_release_all_owners() {
    // Arrange
    let value = r#"{"a":[{"x":1},{"y":"payload"}]}"#;
    let baseline = controls(10 * 1024 * 1024);
    // Assert
    assert!(compare_controlled(value, value, &baseline)
        .expect("baseline")
        .is_some());
    let peak = baseline.peak_query_memory_bytes();
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = controls(budget);
        let result = compare_controlled(value, value, &controls);
        assert_eq!(result.is_ok(), succeeds);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_route_scalar_jsonb_comparisons_through_controls_without_intercepting_arrays() {
    // Arrange
    let op = crate::sql::ast::BinaryOp::JsonbCompare {
        comparison: RowComparisonOp::Eq,
    };
    let text = ScalarValue::Json(format!("\"{}\"", "x".repeat(2_048)));
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    crate::runtime::accounted::json_arena::cancel_comparison_work_once(cancel);
    // Act
    let controlled =
        super::super::binary::binary_scalar_with_controls(&text, &op, &text, Some(&controls));
    let legacy = super::super::binary::binary_scalar_with_controls(&text, &op, &text, None);
    let array = ScalarValue::Array(crate::types::SqlArray::from_elements(vec![]).expect("array"));
    let array_controlled =
        super::super::binary::binary_scalar_with_controls(&array, &op, &text, Some(&controls));
    let array_legacy = super::super::binary::binary_scalar_with_controls(&array, &op, &text, None);
    // Assert
    assert!(controlled.is_err());
    assert!(matches!(legacy, Ok(ScalarValue::Bool(true))));
    assert_eq!(array_controlled.is_err(), array_legacy.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_stack_after_actual_reversal_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let mut value = "0".to_owned();
    for _ in 0..12 {
        value = format!("[{value}]");
    }
    crate::runtime::accounted::json_arena::cancel_comparison_reverse_once(cancel);
    // Act
    let result = compare_controlled(&value, &value, &controls);
    // Assert
    assert!(result.is_err());
    assert!(controls.peak_query_memory_bytes() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
