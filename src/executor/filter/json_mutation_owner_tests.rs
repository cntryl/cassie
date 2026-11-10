use super::{evaluate, Value};
use crate::config::CassieRuntimeLimits;
use crate::executor::filter::EvaluationMemory;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::types::SqlArray;
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
fn path(value: &str) -> Value {
    Value::Array(SqlArray::from_elements(vec![Value::String(value.into())]).expect("path"))
}

#[test]
fn should_retain_workspace_for_mutation_output_variants() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let document = Value::String(r#"{"a":1,"n":null}"#.into());
    let cases = [
        (
            "jsonb_set",
            vec![document.clone(), path("a"), Value::String("2".into())],
        ),
        (
            "jsonb_set_lax",
            vec![
                document.clone(),
                path("a"),
                Value::Null,
                Value::Bool(true),
                Value::String("return_target".into()),
            ],
        ),
        (
            "jsonb_set_lax",
            vec![
                document.clone(),
                path("a"),
                Value::Null,
                Value::Bool(true),
                Value::String("use_json_null".into()),
            ],
        ),
        (
            "jsonb_set_lax",
            vec![
                document.clone(),
                path("a"),
                Value::Null,
                Value::Bool(true),
                Value::String("delete_key".into()),
            ],
        ),
        (
            "jsonb_insert",
            vec![
                Value::String("[1]".into()),
                path("0"),
                Value::String("2".into()),
            ],
        ),
        (
            "jsonb_insert",
            vec![
                Value::String("[1]".into()),
                path("0"),
                Value::String("2".into()),
                Value::Bool(true),
            ],
        ),
        ("json_strip_nulls", vec![document.clone()]),
        ("jsonb_strip_nulls", vec![document.clone()]),
        ("jsonb_pretty", vec![document]),
    ];
    // Act / Assert
    for (name, args) in cases {
        let result = evaluate(name, &args, Some(&controls), Some(&owner))
            .expect("selected")
            .expect("result");
        // Assert
        assert!(matches!(result, Value::String(_)));
        assert!(owner.bytes() > 0);
    }
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_mutation_null_with_lax_error_without_a_retained_lease() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&controls).expect("owner");
    // Act
    let null = evaluate(
        "jsonb_set",
        &[Value::Null, path("a"), Value::String("1".into())],
        Some(&controls),
        Some(&owner),
    )
    .expect("selected");
    let error = evaluate(
        "jsonb_set_lax",
        &[
            Value::String("{}".into()),
            path("a"),
            Value::Null,
            Value::Bool(true),
            Value::String("raise_exception".into()),
        ],
        Some(&controls),
        Some(&owner),
    )
    .expect("selected");
    // Assert
    assert!(matches!(null, Ok(Value::Null)));
    assert!(error.is_err());
    assert_eq!(owner.bytes(), 0);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_mutation_output_after_post_finish_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let owner = EvaluationMemory::new(&controls).expect("owner");
    super::super::json_result_owner::cancel_after_output_once(cancel);
    // Act
    let result = evaluate(
        "jsonb_pretty",
        &[Value::String(r#"{"a":1}"#.into())],
        Some(&controls),
        Some(&owner),
    )
    .expect("selected");
    // Assert
    assert!(result.is_err());
    assert!(controls.peak_query_memory_bytes() > 0);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_grow_or_release_a_synthetic_undersized_mutation_output_owner() {
    // Arrange
    for (budget, succeeds) in [(2_048, true), (1, false)] {
        let controls = controls(budget);
        let owner = EvaluationMemory::new(&controls).expect("owner");
        let mut text = String::with_capacity(1_024);
        text.push('x');
        let capacity = text.capacity();
        let memory = controls.reserve_query_memory(1).expect("memory");
        // Act
        let result = super::super::json_result_owner::retain_output(
            Value::String(text),
            Some(memory),
            Some(&owner),
            Some(&controls),
            "JSON mutation owner uses a different query tracker",
        );
        // Assert
        assert_eq!(result.is_ok(), succeeds);
        if succeeds {
            assert!(owner.bytes() >= capacity);
        }
        drop(result);
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_ownerless_with_general_owner_routes_with_reject_foreign_tracker() {
    // Arrange
    use crate::executor::batch::BatchRow;
    use crate::executor::filter::{evaluate_expr_value_with_context, EvalContext};
    use std::collections::HashMap;
    let owner_controls = controls(10 * 1024 * 1024);
    let foreign = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&owner_controls).expect("owner");
    let general = EvaluationMemory::new(&owner_controls).expect("general");
    let input = [Value::String(r#"{"a":1}"#.into())];
    // Act / Assert
    let ownerless = evaluate("jsonb_pretty", &input, Some(&owner_controls), None)
        .expect("selected")
        .expect("result");
    // Assert
    assert_eq!(owner_controls.current_query_memory_bytes(), 0);
    drop(ownerless);
    let foreign_result =
        evaluate("jsonb_pretty", &input, Some(&foreign), Some(&owner)).expect("selected");
    assert!(foreign_result.is_err());
    assert_eq!(foreign.current_query_memory_bytes(), 0);
    let row = BatchRow::new(vec![]);
    let functions = HashMap::new();
    let context = EvalContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        local_args: None,
        session: None,
        exists: None,
        controls: Some(&owner_controls),
        memory: Some(&general),
        deletion_memory: Some(&owner),
    };
    let expression = crate::sql::parser::parse_expression("jsonb_pretty('{\"a\":1}'::JSONB)")
        .expect("expression");
    let result = evaluate_expr_value_with_context(&row, &expression, context).expect("result");
    assert_eq!(owner.bytes(), 0);
    drop(result);
    drop(general);
    drop(owner);
    assert_eq!(owner_controls.current_query_memory_bytes(), 0);
}
