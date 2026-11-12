use super::{evaluate, Value};
use crate::config::CassieRuntimeLimits;
use crate::executor::filter::EvaluationMemory;
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
fn should_retain_extraction_workspace_for_all_json_path_function_variants() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let input = [
        Value::String(r#"{"a":{"b":"payload"}}"#.into()),
        Value::String("a".into()),
        Value::String("b".into()),
    ];
    // Act / Assert
    for name in [
        "json_extract_path",
        "json_extract_path_text",
        "jsonb_extract_path",
        "jsonb_extract_path_text",
    ] {
        let expected = evaluate(name, &input, Some(&controls), None)
            .expect("selected")
            .expect("legacy");
        let result = evaluate(name, &input, Some(&controls), Some(&owner))
            .expect("selected")
            .expect("result");
        // Assert
        assert_eq!(result, expected);
        assert!(owner.bytes() > 0);
    }
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_variadic_path_output_until_its_owner_is_released() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let input = [
        Value::String(r#"{"a":{"b":"payload"}}"#.into()),
        Value::Array(
            crate::types::SqlArray::from_elements(vec![
                crate::types::Value::String("a".into()),
                crate::types::Value::String("b".into()),
            ])
            .expect("path array"),
        ),
    ];

    // Act
    let result = evaluate(
        "jsonb_extract_path_text",
        &input,
        Some(&controls),
        Some(&owner),
    )
    .expect("selected variadic path")
    .expect("extracted text");

    // Assert
    assert!(matches!(result, Value::String(ref value) if value == "payload"));
    assert!(owner.bytes() > 0);
    drop(result);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_missing_null_with_invalid_path_results_without_retaining_a_lease() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&controls).expect("owner");
    // Act
    let missing = evaluate(
        "jsonb_extract_path_text",
        &[Value::String("{}".into()), Value::String("absent".into())],
        Some(&controls),
        Some(&owner),
    )
    .expect("selected");
    let null = evaluate(
        "jsonb_extract_path_text",
        &[Value::Null, Value::String("a".into())],
        Some(&controls),
        Some(&owner),
    )
    .expect("selected");
    let invalid = evaluate(
        "jsonb_extract_path",
        &[Value::String("[\u{7d}".into()), Value::String("a".into())],
        Some(&controls),
        Some(&owner),
    )
    .expect("selected");
    // Assert
    assert!(matches!(missing, Ok(Value::Null)));
    assert!(matches!(null, Ok(Value::Null)));
    assert!(invalid.is_err());
    assert_eq!(owner.bytes(), 0);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_extracted_output_after_post_finish_cancellation() {
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
        "json_extract_path_text",
        &[
            Value::String(r#"{"a":"payload"}"#.into()),
            Value::Array(
                crate::types::SqlArray::from_elements(vec![
                    crate::types::Value::String("a".into()),
                ])
                .expect("path array"),
            ),
        ],
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
fn should_grow_or_release_a_synthetic_undersized_extraction_owner() {
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
            "JSON path owner uses a different query tracker",
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
fn should_preserve_ownerless_general_owner_with_foreign_tracker_paths() {
    // Arrange
    use crate::executor::batch::BatchRow;
    use crate::executor::filter::{evaluate_expr_value_with_context, EvalContext};
    use std::collections::HashMap;
    let owner_controls = controls(10 * 1024 * 1024);
    let foreign = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&owner_controls).expect("owner");
    let general = EvaluationMemory::new(&owner_controls).expect("general");
    let input = [
        Value::String(r#"{"a":1}"#.into()),
        Value::String("a".into()),
    ];
    // Act / Assert
    let ownerless = evaluate("jsonb_extract_path", &input, Some(&owner_controls), None)
        .expect("selected")
        .expect("result");
    // Assert
    assert_eq!(owner_controls.current_query_memory_bytes(), 0);
    drop(ownerless);
    let foreign_result =
        evaluate("jsonb_extract_path", &input, Some(&foreign), Some(&owner)).expect("selected");
    assert!(foreign_result.is_err());
    assert_eq!(foreign.current_query_memory_bytes(), 0);
    let functions = HashMap::new();
    let row = BatchRow::new(vec![]);
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
    let expression =
        crate::sql::parser::parse_expression("jsonb_extract_path_text('{\"a\":1}'::JSONB, 'a')")
            .expect("expression");
    let result = evaluate_expr_value_with_context(&row, &expression, context).expect("result");
    assert_eq!(owner.bytes(), 0);
    drop(result);
    drop(general);
    drop(owner);
    assert_eq!(owner_controls.current_query_memory_bytes(), 0);
}
