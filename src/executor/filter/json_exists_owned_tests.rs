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

#[test]
fn should_match_value_existence_with_owned_string_backed_key_arrays() {
    // Arrange
    let controls = controls();
    let documents = [r#"{"a":1,"b":2}"#, r#"["a","b",3]"#, r#""a""#];
    let key_arrays = [
        "{a,b}",
        "{missing,NULL}",
        "{}",
        r#"["a",null]"#,
        r#"[{"$serde_json::private::RawValue":"\"a\""}]"#,
    ];
    // Act / Assert
    for document in documents {
        for text in key_arrays {
            for op in [BinaryOp::JsonbKeyExistsAny, BinaryOp::JsonbKeyExistsAll] {
                let left = ScalarValue::Json(document.into());
                let right = ScalarValue::Str(text.into());
                let expected = evaluate(&left, &op, &right).expect("legacy");
                let actual = evaluate_controlled(&left, &op, &right, &controls)
                    .expect("controlled")
                    .expect("owned key array");
                assert!(
                    matches!((actual, expected), (ScalarValue::Bool(a), ScalarValue::Bool(b)) if a == b),
                    "{document} {text}"
                );
                assert_eq!(controls.current_query_memory_bytes(), 0);
            }
        }
    }
}

#[test]
fn should_leave_malformed_string_keys_and_document_errors_on_legacy_paths() {
    // Arrange
    let controls = controls();
    let malformed_keys = ScalarValue::Str("{a,}".into());
    let valid = ScalarValue::Json("{}".into());
    let invalid = ScalarValue::Json("[}".into());
    // Act / Assert
    for document in [&valid, &invalid] {
        let expected = evaluate(document, &BinaryOp::JsonbKeyExistsAny, &malformed_keys)
            .expect_err("legacy error")
            .to_string();
        let candidate = evaluate_controlled(
            document,
            &BinaryOp::JsonbKeyExistsAny,
            &malformed_keys,
            &controls,
        )
        .expect("fallback");
        assert!(candidate.is_none());
        let actual = evaluate(document, &BinaryOp::JsonbKeyExistsAny, &malformed_keys)
            .expect_err("fallback error")
            .to_string();
        assert_eq!(actual, expected);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_enforce_string_backed_key_and_document_owner_overlap() {
    // Arrange
    let left = ScalarValue::Json(r#"{"a":1,"b":2}"#.into());
    let right = ScalarValue::Str(r#"["a","b"]"#.into());
    let baseline = controls();
    let result = evaluate_controlled(&left, &BinaryOp::JsonbKeyExistsAll, &right, &baseline)
        .expect("baseline");
    let peak = baseline.peak_query_memory_bytes();
    drop(result);
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let result = evaluate_controlled(&left, &BinaryOp::JsonbKeyExistsAll, &right, &controls);
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_string_keys_and_document_after_parse_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let left = ScalarValue::Json(r#"{"a":1}"#.into());
    let right = ScalarValue::Str("{a}".into());
    crate::executor::filter::json_delete::cancel_after_owned_path_once(cancel.clone());
    // Act
    let result = evaluate_controlled(&left, &BinaryOp::JsonbKeyExistsAny, &right, &controls);
    // Assert
    assert!(cancel.is_cancelled());
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
