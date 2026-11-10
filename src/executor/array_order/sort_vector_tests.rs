use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::sort::accounting;
use crate::runtime::QueryCancellationHandle;
use crate::types::{ArrayDimension, SqlArray, Vector};
use std::time::Instant;

fn controls(budget: usize, cancel: QueryCancellationHandle) -> QueryExecutionControls {
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: budget,
        query_timeout_ms: 0,
        ..CassieRuntimeLimits::default()
    };
    QueryExecutionControls::with_cancellation(&limits, Instant::now(), cancel)
}

fn fixture(values: Vec<f32>) -> (Value, DataType) {
    let array = SqlArray::new(
        vec![ArrayDimension {
            length: 2,
            lower_bound: -3,
        }],
        vec![Value::Vector(Vector::new(values)), Value::Null],
    )
    .expect("array");
    (
        Value::Array(array),
        DataType::Array(Box::new(DataType::Vector(4096))),
    )
}

#[test]
fn should_cover_wide_vector_expansion_with_match_legacy_key() {
    // Arrange
    let (value, data_type) = fixture(vec![1.0; 4096]);
    let controls = controls(1_000_000, QueryCancellationHandle::new());
    let extra = bound(&value, Some(&data_type), &controls)
        .expect("bound")
        .expect("selected");
    let old_bound = accounting::conversion_bytes(&value, Some(&data_type)).expect("old bound");
    let legacy = super::super::typed_key(&value, Some(&data_type));
    let old_retained = accounting::semantic_heap(&legacy).expect("retained");
    assert!(
        old_bound < old_retained,
        "wide components expose the prior under-admission"
    );
    let memory = controls.reserve_query_memory(extra).expect("key admission");
    // Act
    let (key, retained) = build_observing(&value, &controls, |_| {}).expect("controlled key");
    // Assert
    assert_eq!(key, legacy);
    assert_eq!(retained, old_retained);
    assert!(extra >= retained);
    drop(key);
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_special_floats_nulls_with_shape() {
    // Arrange
    let (value, data_type) = fixture(vec![0.0, -0.0, f32::INFINITY, f32::NEG_INFINITY, f32::NAN]);
    let controls = controls(1_000_000, QueryCancellationHandle::new());
    let extra = bound(&value, Some(&data_type), &controls)
        .expect("bound")
        .expect("selected");
    let memory = controls.reserve_query_memory(extra).expect("admission");
    // Act
    let (key, _) = build_observing(&value, &controls, |_| {}).expect("key");
    // Assert
    assert_eq!(key, super::super::typed_key(&value, Some(&data_type)));
    drop(key);
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_after_an_actual_component_with_release_reservation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = controls(1_000_000, cancellation.clone());
    let (value, data_type) = fixture(vec![1.0; 4096]);
    let extra = bound(&value, Some(&data_type), &controls)
        .expect("bound")
        .expect("selected");
    let memory = controls.reserve_query_memory(extra).expect("admission");
    let mut moved = 0;
    // Act
    let result = build_observing(&value, &controls, |_| {
        moved += 1;
        cancellation.cancel();
    });
    // Assert
    assert_eq!(moved, 1);
    assert!(matches!(
        result,
        Err(crate::app::CassieError::QueryCancelled)
    ));
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_other_array_shapes_on_the_existing_path() {
    // Arrange
    let controls = controls(1_000_000, QueryCancellationHandle::new());
    let (value, data_type) = fixture(vec![1.0]);
    let other = Value::Array(SqlArray::from_elements(vec![Value::Int64(1)]).expect("array"));
    // Act / Assert
    // Assert
    assert!(bound(&value, None, &controls)
        .expect("inferred fallback")
        .is_none());
    assert!(bound(&other, Some(&data_type), &controls)
        .expect("element fallback")
        .is_none());
}

#[test]
fn should_use_search_lineage_before_the_vector_builder() {
    // Arrange
    let (value, data_type) = fixture(vec![1.0; 32]);
    let mut row = crate::executor::batch::BatchRow::new(vec![
        ("v".into(), value.clone()),
        ("\0cassie_cte_lineage:walk".into(), Value::String(r#"{"search_path":[[{"String":"2"}]],"search_column":"v","search_order":"breadth_first","search_key_types":["jsonb"]}"#.into())),
    ]);
    row.set_data_types(vec![data_type.clone(), DataType::Text].into());
    let controls = controls(1_000_000, QueryCancellationHandle::new());
    let expression = crate::sql::ast::Expr::Column("v".into());
    let functions = std::collections::HashMap::new();
    // Act
    let legacy = super::super::key(&row, &expression, &value, &functions, Some(&controls))
        .expect("legacy lineage");
    let (actual, retained) =
        super::super::sort_vector_key(&row, &expression, &value, &functions, &data_type, &controls)
            .expect("controlled lineage");
    // Assert
    assert!(matches!(&actual, SemanticValue::Record(_)));
    assert!(retained.is_none());
    assert_eq!(actual, legacy);
    drop(actual);
    drop(legacy);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
