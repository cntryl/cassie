use super::{estimate, overflow_sum};
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

#[test]
fn should_match_existing_retained_estimate_for_nested_values_and_capacities() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    let mut text = String::with_capacity(256);
    text.push_str("é");
    let mut array = Vec::with_capacity(8);
    array.push(serde_json::Value::String(text));
    let mut emptied = serde_json::Map::new();
    emptied.insert("x".into(), serde_json::Value::Null);
    emptied.remove("x");
    let values = [
        serde_json::Value::Null,
        serde_json::json!(true),
        serde_json::Value::Array(array),
        serde_json::Value::Object(emptied),
        serde_json::json!({"a":[1,{"b":"x"}],"empty":{}}),
    ];
    // Act / Assert
    for value in values {
        assert_eq!(
            estimate(&value, &controls).expect("iterative"),
            crate::runtime::accounted::json::retained_bytes(&value).expect("legacy")
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    assert!(overflow_sum(usize::MAX, 1).is_err());
}

#[test]
fn should_release_estimator_stacks_after_actual_move_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut value = serde_json::Value::Null;
    for _ in 0..16 {
        value = serde_json::Value::Array(vec![value]);
    }
    super::cancel_move_once(cancellation);
    // Act
    let result = estimate(&value, &controls);
    // Assert
    assert!(result.is_err());
    assert!(controls.peak_query_memory_bytes() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_exact_estimator_stack_peak() {
    // Arrange
    let mut value = serde_json::Value::Null;
    for _ in 0..12 {
        value = serde_json::Value::Array(vec![value]);
    }
    let baseline = controls(10 * 1024 * 1024);
    let expected = estimate(&value, &baseline).expect("baseline");
    let peak = baseline.peak_query_memory_bytes();
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = controls(budget);
        let result = estimate(&value, &controls);
        assert_eq!(result.is_ok(), succeeds);
        if let Ok(actual) = result {
            assert_eq!(actual, expected);
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
