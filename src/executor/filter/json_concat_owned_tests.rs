use super::{concatenate, concatenate_controlled};
use crate::config::CassieRuntimeLimits;
use crate::executor::filter::{EvaluationMemory, ScalarValue};
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_match_raw_concat_for_objects_arrays_scalars_with_sentinels() {
    // Arrange
    let controls = controls();
    let cases = [
        (r#"{"a":1,"b":2}"#, r#"{"\u0061":3}"#),
        ("[1,2]", "3"),
        ("1", "2"),
        (r#"{"x":1}"#, "[2]"),
        (r#"{"$serde_json::private::Number":"7"}"#, "{}"),
        (r#"{"a":"\ud800"}"#, r#"{"a":1}"#),
        (r#"{"a":1e999999999}"#, r#"{"a":1}"#),
        (r#"{"a":"\u0000"}"#, r#"{"b":"\u0000"}"#),
    ];
    // Act / Assert
    for (left, right) in cases {
        let left = ScalarValue::Json(left.into());
        let right = ScalarValue::Json(right.into());
        let expected = concatenate(&left, &right).expect("legacy");
        let owner = EvaluationMemory::new(&controls).expect("owner");
        let actual = concatenate_controlled(&left, &right, &controls, &owner)
            .expect("controlled")
            .expect("supported");
        // Assert
        assert!(
            matches!((actual, expected), (ScalarValue::Json(a), ScalarValue::Json(b)) if a == b)
        );
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_concat_document_with_survivor_error_order() {
    // Arrange
    let controls = controls();
    let cases = [
        ("[}", "[}"),
        (r#"{"\ud800":1}"#, r#"{"\udfff":2}"#),
        (r#"{"a":1}"#, r#"{"b":"\ud800"}"#),
        (r#"{"a":1e999999999}"#, "{}"),
    ];
    // Act / Assert
    for (left, right) in cases {
        let left = ScalarValue::Json(left.into());
        let right = ScalarValue::Json(right.into());
        let expected = concatenate(&left, &right)
            .expect_err("legacy error")
            .to_string();
        let owner = EvaluationMemory::new(&controls).expect("owner");
        let actual = concatenate_controlled(&left, &right, &controls, &owner)
            .expect_err("controlled error")
            .to_string();
        assert_eq!(actual, expected);
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_leave_unsupported_concat_operands_to_legacy_dispatch() {
    // Arrange
    let controls = controls();
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let left = ScalarValue::Json("{}".into());
    let right = ScalarValue::Bool(true);
    // Act / Assert
    // Assert
    assert!(concatenate_controlled(&left, &right, &controls, &owner)
        .expect("fallback")
        .is_none());
    assert!(concatenate(&left, &right).is_err());
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_sql_null_with_reject_foreign_output_owner() {
    // Arrange
    let owner_controls = controls();
    let other_controls = controls();
    let owner = EvaluationMemory::new(&owner_controls).expect("owner");
    let left = ScalarValue::Json("{}".into());
    let right = ScalarValue::Json("{}".into());
    // Act / Assert
    // Assert
    assert!(matches!(
        concatenate_controlled(&left, &ScalarValue::Null, &owner_controls, &owner),
        Ok(Some(ScalarValue::Null))
    ));
    assert!(concatenate_controlled(&left, &right, &other_controls, &owner).is_err());
    assert_eq!(other_controls.current_query_memory_bytes(), 0);
    drop(owner);
    assert_eq!(owner_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_concat_owners_after_an_actual_output_copy_is_cancelled() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let left = ScalarValue::Json(r#"{"a":"payload"}"#.into());
    let right = ScalarValue::Json("{}".into());
    crate::executor::filter::json_delete::cancel_concat_after_copy_once(cancellation);
    // Act
    let result = concatenate_controlled(&left, &right, &controls, &owner);
    // Assert
    assert!(result.is_err());
    drop(result);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_exact_concat_overlap_with_release_its_result_owner() {
    // Arrange
    let baseline = controls();
    let left = ScalarValue::Json(r#"{"a":"payload","long":2}"#.into());
    let right = ScalarValue::Json(r#"{"a":3,"z":[1,2]}"#.into());
    let owner = EvaluationMemory::new(&baseline).expect("owner");
    let value = concatenate_controlled(&left, &right, &baseline, &owner)
        .expect("result")
        .expect("supported");
    let peak = baseline.peak_query_memory_bytes();
    // Assert
    assert!(baseline.current_query_memory_bytes() > 0);
    drop(value);
    drop(owner);
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let owner = EvaluationMemory::new(&controls).expect("owner");
        let result = concatenate_controlled(&left, &right, &controls, &owner);
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_retain_a_typed_concat_expression_result() {
    // Arrange
    use crate::executor::batch::BatchRow;
    use crate::executor::filter::{evaluate_expr_value_with_context, EvalContext};
    use crate::types::{DataType, Value};
    use std::collections::HashMap;
    let controls = controls();
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let mut row = BatchRow::new(vec![
        ("a".into(), Value::Json(serde_json::json!({"a":1}))),
        ("b".into(), Value::Json(serde_json::json!({"b":2}))),
    ]);
    row.set_data_types(vec![DataType::Jsonb, DataType::Jsonb].into());
    let expression = crate::sql::parser::parse_expression("a || b").expect("expression");
    let functions = HashMap::new();
    let context = EvalContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        local_args: None,
        session: None,
        exists: None,
        controls: Some(&controls),
        memory: None,
        deletion_memory: Some(&owner),
    };
    // Act
    let result = evaluate_expr_value_with_context(&row, &expression, context).expect("concat");
    // Assert
    let Value::String(text) = &result else {
        panic!("JSON carrier")
    };
    assert_eq!(text, r#"{"a": 1, "b": 2}"#);
    assert!(controls.current_query_memory_bytes() >= text.capacity());
    drop(result);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_unrelated_null_with_array_dispatch_with_an_owner_present() {
    // Arrange
    use crate::executor::batch::BatchRow;
    use crate::executor::filter::{evaluate_expr_value_with_context, EvalContext};
    use crate::types::{SqlArray, Value};
    use std::collections::HashMap;
    let controls = controls();
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let row = BatchRow::new(vec![
        (
            "arr".into(),
            Value::Array(SqlArray::from_elements(vec![Value::Int64(1)]).expect("array")),
        ),
        ("n".into(), Value::Int64(2)),
    ]);
    let functions = HashMap::new();
    let base = EvalContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        local_args: None,
        session: None,
        exists: None,
        controls: Some(&controls),
        memory: None,
        deletion_memory: None,
    };
    // Act / Assert
    for sql in ["arr || NULL", "NULL || arr", "n || NULL", "'x' || NULL"] {
        let expression = crate::sql::parser::parse_expression(sql).expect("expression");
        let legacy = evaluate_expr_value_with_context(&row, &expression, base)
            .map_err(|error| error.to_string());
        let actual = evaluate_expr_value_with_context(
            &row,
            &expression,
            EvalContext {
                deletion_memory: Some(&owner),
                ..base
            },
        )
        .map_err(|error| error.to_string());
        // Assert
        assert_eq!(actual, legacy);
    }
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
