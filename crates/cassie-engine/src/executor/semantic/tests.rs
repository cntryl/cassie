use super::*;
use crate::config::CassieRuntimeLimits;
use crate::types::{ArrayDimension, SqlArray};
use std::time::Instant;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}
fn native(lower: i32, numeric: &str) -> Value {
    Value::Array(
        SqlArray::new(
            vec![ArrayDimension {
                length: 3,
                lower_bound: lower,
            }],
            vec![
                Value::Json(serde_json::from_str(numeric).expect("number")),
                Value::Json(serde_json::Value::Null),
                Value::Null,
            ],
        )
        .expect("native array"),
    )
}

#[test]
fn should_select_jsonb_keys_only_with_declared_identity() {
    // Arrange
    let controls = controls();
    let text = Value::String("1.00".into());
    let document = Value::Json(serde_json::from_str("1e0").expect("JSON number"));
    // Act
    let jsonb = typed_value(&text, Some(&DataType::Jsonb), Some(&controls)).expect("JSONB key");
    let native =
        typed_value(&document, Some(&DataType::Jsonb), Some(&controls)).expect("native JSONB");
    let ordinary = typed_value(&text, Some(&DataType::Text), Some(&controls)).expect("text key");
    // Assert
    assert_eq!(jsonb, native);
    assert!(matches!(ordinary,SemanticValue::String(ref value) if value=="1.00"));
    assert_ne!(jsonb, ordinary);
}

#[test]
fn should_distinguish_sql_null_from_document_null_in_jsonb_keys() {
    // Arrange
    let controls = controls();
    // Act
    let sql_null =
        typed_value(&Value::Null, Some(&DataType::Jsonb), Some(&controls)).expect("SQL null");
    let document_null = typed_value(
        &Value::Json(serde_json::Value::Null),
        Some(&DataType::Jsonb),
        Some(&controls),
    )
    .expect("document null");
    // Assert
    assert!(sql_null.is_null());
    assert!(!document_null.is_null());
    assert_ne!(sql_null, document_null);
}

#[test]
fn should_preserve_native_jsonb_array_identity_after_parent_values_drop() {
    // Arrange
    let controls = controls();
    let data_type = DataType::Array(Box::new(DataType::Jsonb));
    let first = native(-2, "1.0");
    let equivalent = native(-2, "1e0");
    let shifted = native(-1, "1");
    // Act
    let key = typed_value(&first, Some(&data_type), Some(&controls)).expect("array key");
    let same = typed_value(&equivalent, Some(&data_type), Some(&controls)).expect("same key");
    let different = typed_value(&shifted, Some(&data_type), Some(&controls)).expect("shifted key");
    let clone = key.clone();
    drop(first);
    drop(equivalent);
    drop(shifted);
    drop(key);
    let retained = controls.current_query_memory_bytes();
    // Assert
    assert_eq!(clone, same);
    assert_ne!(clone, different);
    assert!(retained > 0);
    drop(clone);
    drop(same);
    drop(different);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_document_null_in_legacy_jsonb_array_keys() {
    // Arrange
    let controls = controls();
    let data_type = DataType::Array(Box::new(DataType::Jsonb));
    let carrier = Value::Json(serde_json::from_str("[1.00,null]").expect("legacy documents"));
    let expected = Value::Array(
        SqlArray::from_elements(vec![
            Value::Json(serde_json::json!(1)),
            Value::Json(serde_json::Value::Null),
        ])
        .expect("array"),
    );
    // Act
    let legacy = typed_value(&carrier, Some(&data_type), Some(&controls)).expect("legacy key");
    let native = typed_value(&expected, Some(&data_type), Some(&controls)).expect("native key");
    // Assert
    assert_eq!(legacy, native);
}

#[test]
fn should_preserve_sql_null_origins_in_text_jsonb_array_keys() {
    // Arrange
    let controls = controls();
    let data_type = DataType::Array(Box::new(DataType::Jsonb));
    let text = Value::String(r#"[-2:0]={1.00,"null",NULL}"#.into());
    // Act
    let parsed = typed_value(&text, Some(&data_type), Some(&controls)).expect("text key");
    let native =
        typed_value(&native(-2, "1"), Some(&data_type), Some(&controls)).expect("native key");
    // Assert
    assert_eq!(parsed, native);
}

#[test]
fn should_release_admitted_jsonb_array_keys_after_a_late_invalid_element() {
    // Arrange
    let controls = controls();
    let array = Value::Array(
        SqlArray::from_elements(vec![
            Value::Json(serde_json::json!({"ok":1})),
            Value::String("invalid".into()),
        ])
        .expect("fixture array"),
    );
    let data_type = DataType::Array(Box::new(DataType::Jsonb));
    // Act
    let result = typed_value(&array, Some(&data_type), Some(&controls));
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
