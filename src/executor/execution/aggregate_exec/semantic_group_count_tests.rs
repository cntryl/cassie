use super::*;
use crate::executor::semantic::SemanticValue;
use crate::runtime::QueryCancellationHandle;

fn nested_key() -> SemanticKey {
    let mut value = SemanticValue::String("payload".into());
    for _ in 0..40 {
        value = SemanticValue::Array(vec![value]);
    }
    SemanticKey::from_semantic_values(vec![value])
}

#[test]
fn should_preserve_nested_group_estimate_at_exact_scratch_budget() {
    // Arrange
    let key = nested_key();
    let values = vec![("field".to_string(), Value::String("value".into()))];
    let controls = controls_with_budget(1_000_000);
    let expected = key
        .estimated_bytes()
        .saturating_add(group_memory::json_bytes(&values));
    // Act
    let actual = group_memory::serial_group_bytes(&key, &values, &controls).expect("estimate");
    // Assert
    assert_eq!(actual, expected);
    let peak = controls.peak_query_memory_bytes();
    assert!(peak > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = controls_with_budget(budget);
        let result = group_memory::serial_group_bytes(&key, &values, &controls);
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_estimator_scratch_after_cancellation_during_group_key_walk() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let key = nested_key();
    crate::types::semantic::cancel_next_estimate_frame(cancellation);
    // Act
    let result = group_memory::serial_group_bytes(&key, &[], &controls);
    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
