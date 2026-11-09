use super::{evaluate, evaluate_controlled, BinaryOp, ScalarValue};
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::types::{SqlArray, Value};
use std::time::Instant;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

fn keys(values: Vec<Value>) -> ScalarValue {
    ScalarValue::Array(SqlArray::from_elements(values).expect("native keys"))
}

#[test]
fn should_match_value_existence_for_duplicate_strings_and_null_keys() {
    // Arrange
    let controls = controls();
    let cases = [
        (
            r#"{"a":1,"\u0061":2}"#,
            BinaryOp::JsonbKeyExists,
            ScalarValue::Str("a".into()),
        ),
        (
            r#"["x",1,null]"#,
            BinaryOp::JsonbKeyExists,
            ScalarValue::Str("x".into()),
        ),
        (
            r#""x""#,
            BinaryOp::JsonbKeyExists,
            ScalarValue::Str("x".into()),
        ),
        ("3", BinaryOp::JsonbKeyExists, ScalarValue::Str("3".into())),
        (
            r#"{"$serde_json::private::RawValue":"\"x\""}"#,
            BinaryOp::JsonbKeyExists,
            ScalarValue::Str("x".into()),
        ),
        (
            r#"{"$serde_json::private::Number":"7"}"#,
            BinaryOp::JsonbKeyExists,
            ScalarValue::Str("7".into()),
        ),
        (
            "null",
            BinaryOp::JsonbKeyExists,
            ScalarValue::Str("null".into()),
        ),
        ("{}", BinaryOp::JsonbKeyExistsAny, keys(vec![Value::Null])),
        ("{}", BinaryOp::JsonbKeyExistsAll, keys(vec![Value::Null])),
        ("{}", BinaryOp::JsonbKeyExistsAny, keys(vec![])),
        ("{}", BinaryOp::JsonbKeyExistsAll, keys(vec![])),
        (
            r#"{"a":1}"#,
            BinaryOp::JsonbKeyExistsAll,
            keys(vec![Value::String("a".into()), Value::Null]),
        ),
    ];
    // Act / Assert
    for (source, op, rhs) in cases {
        let left = ScalarValue::Json(source.into());
        let expected = evaluate(&left, &op, &rhs).expect("legacy");
        let actual = evaluate_controlled(&left, &op, &rhs, &controls)
            .expect("controlled")
            .expect("supported");
        assert!(
            matches!((actual, expected), (ScalarValue::Bool(a), ScalarValue::Bool(b)) if a == b)
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_value_sentinel_and_document_before_operand_errors() {
    // Arrange
    let controls = controls();
    let invalid_documents = ["[}", r#"{"$serde_json::private::Number":"bad"}"#];
    let incompatible = ScalarValue::Bool(true);
    // Act / Assert
    for source in invalid_documents {
        let left = ScalarValue::Json(source.into());
        let expected = evaluate(&left, &BinaryOp::JsonbKeyExists, &incompatible)
            .expect_err("legacy error")
            .to_string();
        let candidate =
            evaluate_controlled(&left, &BinaryOp::JsonbKeyExists, &incompatible, &controls)
                .expect("fallback");
        assert!(candidate.is_none());
        let actual = evaluate(&left, &BinaryOp::JsonbKeyExists, &incompatible)
            .expect_err("fallback error")
            .to_string();
        assert_eq!(actual, expected);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_validate_all_native_keys_before_short_circuiting() {
    // Arrange
    let controls = controls();
    let left = ScalarValue::Json(r#"{"a":1}"#.into());
    let rhs = keys(vec![Value::String("a".into()), Value::Int64(1)]);
    // Act / Assert
    assert!(
        evaluate_controlled(&left, &BinaryOp::JsonbKeyExistsAny, &rhs, &controls)
            .expect("fallback")
            .is_none()
    );
    assert!(evaluate(&left, &BinaryOp::JsonbKeyExistsAny, &rhs).is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_arena_after_precancelled_existence() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let left = ScalarValue::Json(r#"{"a":1}"#.into());
    let rhs = ScalarValue::Str("a".into());
    cancellation.cancel();
    // Act
    let result = evaluate_controlled(&left, &BinaryOp::JsonbKeyExists, &rhs, &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
