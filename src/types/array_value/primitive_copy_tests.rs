//! Checked primitive SQL Array copy and raw-shape preservation.
use super::primitive_copy::{copy_primitive_array, PrimitiveCopyProgress};
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

#[test]
fn should_preserve_array_dimensions_values_and_float_bits() {
    // Arrange
    let nan = f64::from_bits(0x7ff8_0000_0000_1234);
    let array = SqlArray::new(
        vec![ArrayDimension {
            length: 5,
            lower_bound: -7,
        }],
        vec![
            Value::Null,
            Value::Bool(true),
            Value::Int64(i64::MIN),
            Value::Float64(-0.0),
            Value::Float64(nan),
        ],
    )
    .expect("array");
    let controls = controls(1_000_000);
    let source = controls.reserve_query_memory(64).expect("source");

    // Act
    let output = copy_primitive_array(&array, &source, &controls, |_| {})
        .expect("copy")
        .expect("primitive");
    let (copy, memory) = output.into_parts();

    // Assert
    assert_eq!(copy.dimensions(), array.dimensions());
    assert_eq!(copy.elements()[..3], array.elements()[..3]);
    assert!(matches!(copy.elements()[3], Value::Float64(v) if v.to_bits() == (-0.0_f64).to_bits()));
    assert!(matches!(copy.elements()[4], Value::Float64(v) if v.to_bits() == nan.to_bits()));
    assert!(memory.belongs_to(&controls));
    drop(copy);
    drop(memory);
    drop(source);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_malformed_deserialized_array_shape() {
    // Arrange
    let array: SqlArray = serde_json::from_value(
        serde_json::json!({"dimensions":[{"length":2,"lower_bound":1}], "elements":[]}),
    )
    .expect("deserialize malformed layout");
    let controls = controls(4096);
    let source = controls.reserve_query_memory(0).expect("source");

    // Act
    let output = copy_primitive_array(&array, &source, &controls, |_| {})
        .expect("copy")
        .expect("primitive");
    let (copy, memory) = output.into_parts();

    // Assert
    assert_eq!(copy, array);
    assert_eq!(copy.dimensions()[0].length, 2);
    assert!(copy.elements().is_empty());
    drop(copy);
    drop(memory);
    drop(source);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_nonprimitive_array_families_on_the_existing_path() {
    // Arrange
    let controls = controls(4096);
    let source = controls.reserve_query_memory(0).expect("source");
    let arrays = [
        SqlArray::from_elements(vec![Value::String("x".into())]).expect("string"),
        SqlArray::from_elements(vec![Value::Array(SqlArray::empty())]).expect("nested"),
        SqlArray::from_elements(vec![Value::Json(serde_json::json!({"a": 1}))]).expect("json"),
        SqlArray::from_elements(vec![Value::Typed(crate::types::typed::TypedValue::Date(0))])
            .expect("typed"),
    ];

    // Act / Assert
    for array in arrays {
        assert!(copy_primitive_array(&array, &source, &controls, |_| {})
            .expect("inspect")
            .is_none());
    }
    drop(source);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_during_actual_dimension_and_element_copy() {
    // Arrange
    let array = SqlArray::new(
        vec![ArrayDimension {
            length: 2,
            lower_bound: 1,
        }],
        vec![Value::Int64(i64::MIN), Value::Int64(i64::MAX)],
    )
    .expect("array");
    for cancel_dimension in [true, false] {
        let cancel = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancel.clone(),
        );
        let source = controls.reserve_query_memory(64).expect("source");
        let mut seen = false;

        // Act
        let result = copy_primitive_array(&array, &source, &controls, |progress| {
            if matches!(
                (cancel_dimension, progress),
                (true, PrimitiveCopyProgress::Dimension) | (false, PrimitiveCopyProgress::Element)
            ) {
                seen = true;
                cancel.cancel();
            }
        });

        // Assert
        assert!(seen);
        assert!(matches!(
            result,
            Err(crate::app::CassieError::QueryCancelled)
        ));
        assert_eq!(controls.current_query_memory_bytes(), 64);
        drop(source);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_reject_a_foreign_array_source_tracker() {
    // Arrange
    let left = controls(4096);
    let right = controls(4096);
    let source = left.reserve_query_memory(32).expect("source");

    // Act
    let result = copy_primitive_array(&SqlArray::empty(), &source, &right, |_| {});

    // Assert
    assert!(matches!(
        result,
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(left.current_query_memory_bytes(), 32);
    assert_eq!(right.current_query_memory_bytes(), 0);
    drop(source);
    assert_eq!(left.current_query_memory_bytes(), 0);
}
