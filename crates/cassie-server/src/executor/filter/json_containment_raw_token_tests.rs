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
fn contains() -> BinaryOp {
    BinaryOp::Typed {
        operator: TypedOperator::ContainsOrEqual,
    }
}

#[test]
fn should_match_legacy_raw_pair_syntax_whitespace_with_nested_values() {
    // Arrange
    let cases = [
        ("  [1,2] \n", " [2] "),
        ("1.", "2."),
        ("1", "2."),
        (r#""\uX123""#, "1"),
        ("1", r#""\uX123""#),
        (
            r#"[{"$serde_json::private::Number":"7"}]"#,
            r#"[{"$serde_json::private::Number":"7"}]"#,
        ),
        (r#"[{"a":[1,2]}]"#, r#"[{"a":[2]}]"#),
    ];
    let controls = controls(10 * 1024 * 1024);
    // Act / Assert
    for (left, right) in cases {
        let left = ScalarValue::Json(left.into());
        let right = ScalarValue::Json(right.into());
        let expected = evaluate_with_controls(&contains(), &left, &right, None)
            .map_err(|error| error.to_string());
        let actual = evaluate_with_controls(&contains(), &left, &right, Some(&controls))
            .map_err(|error| error.to_string());
        match (expected, actual) {
            // Assert
            (Ok(ScalarValue::Bool(a)), Ok(ScalarValue::Bool(b))) => assert_eq!(a, b),
            (Err(a), Err(b)) => assert_eq!(a, b),
            _ => panic!("raw pair parity"),
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_pair_after_actual_scan_with_either_token_copy_cancellation() {
    // Arrange
    for phase in 0..3 {
        let cancel = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancel.clone(),
        );
        let candidate = ScalarValue::Json(if phase == 2 {
            "1".into()
        } else {
            format!("\"{}\"", "é".repeat(2_048))
        });
        let required = ScalarValue::Json(format!("\"{}\"", "x".repeat(2_048)));
        if phase == 0 {
            crate::executor::filter::json_delete::cancel_containment_token_scan_once(cancel);
        } else {
            crate::executor::filter::json_delete::cancel_containment_token_copy_once(
                cancel,
                if phase == 1 { 1 } else { 2 },
            );
        }
        // Act
        let result = evaluate_with_controls(&contains(), &candidate, &required, Some(&controls));
        // Assert
        assert!(result.is_err());
        assert!(controls.peak_query_memory_bytes() > 0);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_admit_measured_raw_pair_with_scanner_overlap() {
    // Arrange
    let value = ScalarValue::Json("[[1,2],[3,4]]".into());
    let baseline = controls(10 * 1024 * 1024);
    // Assert
    assert!(matches!(
        evaluate_with_controls(&contains(), &value, &value, Some(&baseline)),
        Ok(ScalarValue::Bool(true))
    ));
    let peak = baseline.peak_query_memory_bytes();
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = controls(budget);
        let result = evaluate_with_controls(&contains(), &value, &value, Some(&controls));
        assert_eq!(result.is_ok(), succeeds);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
