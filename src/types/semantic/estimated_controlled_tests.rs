//! Len-based semantic group estimate parity and resource probes.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;
use crate::types::semantic::{JsonbArrayKey, JsonbKey, RecordKey};
use crate::types::{SqlArray, Value};
use std::time::Instant;

#[test]
fn should_match_existing_len_estimate_for_all_semantic_shapes() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let mut text = String::with_capacity(1024);
    text.push('Ω');
    let jsonb = JsonbKey::from_text("{}", None).expect("jsonb");
    let array = SqlArray::from_elements(vec![Value::Null]).expect("array");
    let jsonb_array =
        JsonbArrayKey::build(&array, None, |_| Ok(SemanticValue::Null)).expect("JSONB array");
    let record = RecordKey::build(None, 64, || {
        Ok(Some((SemanticValue::String("x".into()), 1)))
    })
    .expect("record")
    .expect("value");
    let key = SemanticKey::from_semantic_values(vec![
        SemanticValue::Null,
        SemanticValue::Bool(true),
        SemanticValue::from_value(&Value::Int64(1)),
        SemanticValue::String(text),
        SemanticValue::Json("{}".into()),
        SemanticValue::Vector(vec![1, 2]),
        SemanticValue::Typed(crate::types::typed::TypedKey::Bits(vec![1, 2], 9)),
        SemanticValue::Jsonb(jsonb),
        SemanticValue::JsonbArray(jsonb_array),
        SemanticValue::Record(record),
        SemanticValue::Array(vec![SemanticValue::String("nested".into())]),
        SemanticValue::SqlArray {
            dimensions: vec![(2, -4)],
            elements: vec![
                SemanticValue::Null,
                SemanticValue::Array(vec![SemanticValue::Bool(false)]),
            ],
        },
    ]);

    // Act
    let actual = key.estimated_bytes_controlled(&controls).expect("estimate");
    // Assert
    assert_eq!(actual, key.estimated_bytes());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_estimate_stack_after_actual_frame_move_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let value = SemanticValue::Null;
    let mut stack = Stack::new(&controls).expect("stack");
    stack.push(Frame::Value(&value)).expect("first");
    stack.push(Frame::Value(&value)).expect("second");
    assert_eq!(stack.frames.len(), stack.frames.capacity());
    let original = controls.current_query_memory_bytes();
    let mut moved = 0;
    // Act
    let result = stack.push_observing(Frame::Value(&value), || {
        moved += 1;
        cancellation.cancel();
    });
    // Assert
    assert_eq!(moved, 1);
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(controls.current_query_memory_bytes(), original);
    drop(stack);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_deep_semantic_estimate_frame_budget() {
    // Arrange
    let mut value = SemanticValue::String("payload".into());
    for _ in 0..40 {
        value = SemanticValue::Array(vec![value]);
    }
    let key = SemanticKey::from_semantic_values(vec![value]);
    let baseline =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    assert_eq!(
        key.estimated_bytes_controlled(&baseline).expect("measure"),
        key.estimated_bytes()
    );
    let required = baseline.peak_query_memory_bytes();
    assert!(required > 0);
    for (budget, accepted) in [(required - 1, false), (required, true)] {
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: budget,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        // Act
        let result = key.estimated_bytes_controlled(&controls);
        // Assert
        assert_eq!(result.is_ok(), accepted);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_report_root_estimate_overflow_as_resource_limit() {
    // Arrange
    // Act
    let result = checked_root_add(usize::MAX, 1);
    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
}
