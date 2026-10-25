use super::excluded_local_args;
use crate::catalog::{CollectionSchema, FieldMeta};
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::types::{DataType, Value};
use std::time::Instant;

fn schema() -> CollectionSchema {
    CollectionSchema {
        collection: "excluded_scope".into(),
        fields: vec![FieldMeta {
            name: "nullable".into(),
            data_type: DataType::Jsonb,
            is_indexed: false,
            boost: None,
        }],
    }
}

#[test]
fn should_admit_excluded_scope_given_bounded_memory_when_null_fields_are_materialized() {
    // Arrange
    let controls = |budget| {
        QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: budget,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        )
    };
    let scarce = controls(64);
    let ample = controls(8192);
    let payload = serde_json::json!({});

    // Act
    let denied = excluded_local_args(&payload, &schema(), &scarce);
    let admitted = excluded_local_args(&payload, &schema(), &ample).expect("admitted scope");

    // Assert
    assert!(denied
        .err()
        .expect("scope exceeds budget")
        .to_string()
        .contains("memory budget"));
    assert_eq!(scarce.current_query_memory_bytes(), 0);
    assert_eq!(admitted.values.get("excluded.nullable"), Some(&Value::Null));
    assert!(ample.current_query_memory_bytes() > 0);
    drop(admitted);
    assert_eq!(ample.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_excluded_scope_given_cancellation_before_materializing_fields() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();

    // Act
    let result = excluded_local_args(&serde_json::json!({}), &schema(), &controls);

    // Assert
    assert!(result
        .err()
        .expect("cancelled scope")
        .to_string()
        .contains("query canceled"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
