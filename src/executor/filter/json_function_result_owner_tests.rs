use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::batch::BatchRow;
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
fn should_preserve_function_result_variants_with_scalar_errors() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&controls).expect("owner");
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
    let values = [
        Value::String(r#"{"raw":1}"#.into()),
        Value::Json(serde_json::json!({"a":1})),
        Value::Null,
        Value::Bool(true),
        Value::Int64(3),
        Value::Float64(f64::NAN),
        Value::Array(crate::types::SqlArray::from_elements(vec![]).expect("array")),
    ];
    // Act / Assert
    for value in values {
        let expected = match &value {
            Value::String(text) => Ok(ScalarValue::Json(text.clone())),
            Value::Null => Ok(ScalarValue::Null),
            other => json_concat::as_json_document(&scalar_from_value(other)),
        }
        .map_err(|error| error.to_string());
        let actual = function_json_result(value, context).map_err(|error| error.to_string());
        match (expected, actual) {
            (Ok(ScalarValue::Json(a)), Ok(ScalarValue::Json(b))) | (Err(a), Err(b)) => {
                // Assert
                assert_eq!(a, b);
            }
            (Ok(ScalarValue::Null), Ok(ScalarValue::Null)) => {}
            _ => panic!("function result parity"),
        }
    }
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_builtin_jsonpath_result_with_call_function_once() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let row = BatchRow::new(vec![(
        "doc".into(),
        Value::Json(serde_json::json!({"a":["one","two"]})),
    )]);
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
    let expression =
        crate::sql::parser::parse_expression("jsonb_path_query_array(doc, '$.a[*]') ->> 0")
            .expect("expression");
    reset_json_function_call_count();
    // Act
    let result = evaluate_expr_value_with_context(&row, &expression, context).expect("result");
    // Assert
    assert!(matches!(&result, Value::String(text) if text == "one"));
    assert_eq!(take_json_function_call_count(), 1);
    assert!(controls.current_query_memory_bytes() > 0);
    drop(result);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_user_json_function_with_null_or_error_once() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    let row = BatchRow::new(vec![]);
    for (body, expected) in [
        ("'{\"a\":\"ok\"}'::JSONB", Some("ok")),
        ("NULL", None),
        ("1 / 0", None),
    ] {
        let owner = EvaluationMemory::new(&controls).expect("owner");
        let functions = HashMap::from([(
            "f".into(),
            crate::catalog::FunctionMeta {
                name: "f".into(),
                args: vec![],
                return_type: crate::types::DataType::Jsonb,
                volatility: crate::catalog::Volatility::Immutable,
                body: body.into(),
                object_id: 0,
            },
        )]);
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
        let expression = crate::sql::parser::parse_expression("f() ->> 'a'").expect("expression");
        reset_json_function_call_count();
        // Act
        let result = evaluate_expr_value_with_context(&row, &expression, context);
        // Assert
        if body == "1 / 0" {
            assert!(result.is_err());
        } else if let Some(expected) = expected {
            assert!(matches!(&result, Ok(Value::String(text)) if text == expected));
        } else {
            assert!(matches!(&result, Ok(Value::Null)));
        }
        assert_eq!(take_json_function_call_count(), 1);
        drop(result);
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_cancel_during_actual_builtin_result_copy_with_release_owners() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let row = BatchRow::new(vec![(
        "doc".into(),
        Value::Json(serde_json::json!({"a":["x".repeat(2_048)]})),
    )]);
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
    let expression =
        crate::sql::parser::parse_expression("jsonb_path_query_array(doc, '$.a[*]') ->> 0")
            .expect("expression");
    json_scalar_leaf::cancel_copy_once(cancellation, 3);
    // Act
    let result = evaluate_expr_value_with_context(&row, &expression, context);
    // Assert
    assert!(result.is_err());
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_function_general_owner_with_ownerless_fallbacks() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    let general = EvaluationMemory::new(&controls).expect("general");
    let deletion = EvaluationMemory::new(&controls).expect("deletion");
    let functions = HashMap::new();
    let context = EvalContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        local_args: None,
        session: None,
        exists: None,
        controls: Some(&controls),
        memory: Some(&general),
        deletion_memory: Some(&deletion),
    };
    // Act / Assert
    for context in [
        context,
        EvalContext {
            memory: None,
            deletion_memory: None,
            ..context
        },
        EvalContext {
            memory: None,
            controls: None,
            ..context
        },
    ] {
        let result =
            function_json_result(Value::Json(serde_json::json!({"a":1})), context).expect("result");
        // Assert
        assert!(matches!(result, ScalarValue::Json(text) if text == r#"{"a":1}"#));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_admit_measured_function_result_output_overlap() {
    // Arrange
    let baseline = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&baseline).expect("owner");
    let functions = HashMap::new();
    let value = Value::Json(serde_json::json!(["x".repeat(1_024), {"a":1}]));
    let context = EvalContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        local_args: None,
        session: None,
        exists: None,
        controls: Some(&baseline),
        memory: None,
        deletion_memory: Some(&owner),
    };
    let result = function_json_result(value.clone(), context).expect("baseline");
    let peak = baseline.peak_query_memory_bytes();
    drop(result);
    drop(owner);
    // Assert
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = controls(budget);
        let owner = EvaluationMemory::new(&controls).expect("owner");
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
        let result = function_json_result(value.clone(), context);
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_reject_a_foreign_function_result_owner_without_retaining_output() {
    // Arrange
    let owner_controls = controls(10 * 1024 * 1024);
    let run_controls = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&owner_controls).expect("owner");
    let functions = HashMap::new();
    let context = EvalContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        local_args: None,
        session: None,
        exists: None,
        controls: Some(&run_controls),
        memory: None,
        deletion_memory: Some(&owner),
    };
    // Act
    let result = function_json_result(Value::Json(serde_json::json!({"a":1})), context);
    // Assert
    assert!(result.is_err());
    assert_eq!(run_controls.current_query_memory_bytes(), 0);
    drop(owner);
    assert_eq!(owner_controls.current_query_memory_bytes(), 0);
}
