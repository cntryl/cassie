//! Function-form JSON path extraction with one expression owner.
use super::{evaluate_expr_value_with_context, EvalContext, EvaluationMemory, Value};
use crate::config::CassieRuntimeLimits;
use crate::executor::batch::BatchRow;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::collections::HashMap;
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
fn should_preserve_all_path_function_results_with_a_shared_owner() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let functions = HashMap::new();
    let row = BatchRow::new(vec![(
        "doc".into(),
        Value::String(r#"{"a":"payload"}"#.into()),
    )]);
    let context = EvalContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        local_args: None,
        session: None,
        exists: None,
        controls: Some(&controls),
        memory: Some(&owner),
        deletion_memory: Some(&owner),
    };

    // Act / Assert
    for name in [
        "json_extract_path",
        "json_extract_path_text",
        "jsonb_extract_path",
        "jsonb_extract_path_text",
    ] {
        let expression =
            crate::sql::parser::parse_expression(&format!("{name}(doc, 'a')")).expect("expression");
        let legacy = evaluate_expr_value_with_context(
            &row,
            &expression,
            EvalContext {
                memory: None,
                deletion_memory: None,
                controls: None,
                ..context
            },
        )
        .expect("legacy result");
        let actual =
            evaluate_expr_value_with_context(&row, &expression, context).expect("owned result");
        // Assert
        assert_eq!(actual, legacy, "{name}");
        assert!(owner.bytes() > 0, "{name} must retain admission");
    }
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_during_the_selected_shared_owner_function_scan() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let functions = HashMap::new();
    let row = BatchRow::new(vec![(
        "doc".into(),
        Value::String(r#"{"a":"payload"}"#.into()),
    )]);
    let expression = crate::sql::parser::parse_expression("jsonb_extract_path_text(doc, 'a')")
        .expect("expression");
    let context = EvalContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        local_args: None,
        session: None,
        exists: None,
        controls: Some(&controls),
        memory: Some(&owner),
        deletion_memory: Some(&owner),
    };
    super::json_delete::cancel_function_path_scan_once(cancel.clone());

    // Act
    let result = evaluate_expr_value_with_context(&row, &expression, context);

    // Assert
    assert!(cancel.is_canceled(), "actual raw scan must reach the hook");
    assert!(result.is_err());
    drop(result);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reach_the_controlled_scan_for_each_shared_owner_path_variant() {
    // Arrange
    let functions = HashMap::new();
    let row = BatchRow::new(vec![(
        "doc".into(),
        Value::String(r#"{"a":"payload"}"#.into()),
    )]);
    for name in [
        "json_extract_path",
        "json_extract_path_text",
        "jsonb_extract_path",
        "jsonb_extract_path_text",
    ] {
        let cancel = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancel.clone(),
        );
        let owner = EvaluationMemory::new(&controls).expect("owner");
        let expression =
            crate::sql::parser::parse_expression(&format!("{name}(doc, 'a')")).expect("expression");
        let context = EvalContext {
            params: &[],
            search_context: None,
            user_functions: &functions,
            local_args: None,
            session: None,
            exists: None,
            controls: Some(&controls),
            memory: Some(&owner),
            deletion_memory: Some(&owner),
        };
        super::json_delete::cancel_function_path_scan_once(cancel.clone());

        // Act
        let result = evaluate_expr_value_with_context(&row, &expression, context);

        // Assert
        assert!(
            cancel.is_canceled(),
            "{name} must reach the controlled scan"
        );
        assert!(result.is_err());
        drop(result);
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_shared_owner_after_actual_function_output_copy_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let functions = HashMap::new();
    let row = BatchRow::new(vec![(
        "doc".into(),
        Value::String(r#"{"a":"payload"}"#.into()),
    )]);
    let expression = crate::sql::parser::parse_expression("jsonb_extract_path_text(doc, 'a')")
        .expect("expression");
    let context = EvalContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        local_args: None,
        session: None,
        exists: None,
        controls: Some(&controls),
        memory: Some(&owner),
        deletion_memory: Some(&owner),
    };
    super::json_delete::cancel_function_path_copy_once(cancel.clone());

    // Act
    let result = evaluate_expr_value_with_context(&row, &expression, context);

    // Assert
    assert!(
        cancel.is_canceled(),
        "actual output copy must reach the hook"
    );
    assert!(result.is_err());
    drop(result);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_a_foreign_owner_with_release_both_query_trackers() {
    // Arrange
    let owner_controls = controls(10 * 1024 * 1024);
    let run_controls = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&owner_controls).expect("owner");
    let functions = HashMap::new();
    let row = BatchRow::new(vec![(
        "doc".into(),
        Value::String(r#"{"a":"payload"}"#.into()),
    )]);
    let expression = crate::sql::parser::parse_expression("jsonb_extract_path_text(doc, 'a')")
        .expect("expression");
    let context = EvalContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        local_args: None,
        session: None,
        exists: None,
        controls: Some(&run_controls),
        memory: Some(&owner),
        deletion_memory: Some(&owner),
    };

    // Act
    let result = evaluate_expr_value_with_context(&row, &expression, context);

    // Assert
    assert!(result.is_err());
    drop(result);
    drop(owner);
    assert_eq!(owner_controls.current_query_memory_bytes(), 0);
    assert_eq!(run_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_null_before_invalid_document_with_release_failed_work() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let functions = HashMap::new();
    let row = BatchRow::new(vec![("doc".into(), Value::String("[\u{7d}".into()))]);
    let context = EvalContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        local_args: None,
        session: None,
        exists: None,
        controls: Some(&controls),
        memory: Some(&owner),
        deletion_memory: Some(&owner),
    };
    let null_path = crate::sql::parser::parse_expression("json_extract_path_text(doc, NULL)")
        .expect("NULL path");
    let invalid = crate::sql::parser::parse_expression("json_extract_path_text(doc, 'a')")
        .expect("invalid document");

    // Act
    let null_result = evaluate_expr_value_with_context(&row, &null_path, context);
    let invalid_result = evaluate_expr_value_with_context(&row, &invalid, context);

    // Assert
    assert!(matches!(null_result, Ok(Value::Null)));
    assert!(invalid_result.is_err());
    drop(invalid_result);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_measured_shared_owner_extraction_overlap() {
    // Arrange
    let baseline = controls(10 * 1024 * 1024);
    let functions = HashMap::new();
    let row = BatchRow::new(vec![(
        "doc".into(),
        Value::String(r#"{"a":"payload"}"#.into()),
    )]);
    let expression = crate::sql::parser::parse_expression("jsonb_extract_path_text(doc, 'a')")
        .expect("expression");
    let run = |controls: &QueryExecutionControls| {
        let owner = EvaluationMemory::new(controls).expect("owner");
        let context = EvalContext {
            params: &[],
            search_context: None,
            user_functions: &functions,
            local_args: None,
            session: None,
            exists: None,
            controls: Some(controls),
            memory: Some(&owner),
            deletion_memory: Some(&owner),
        };
        let result = evaluate_expr_value_with_context(&row, &expression, context);
        let succeeded = result.is_ok();
        drop(result);
        drop(owner);
        succeeded
    };
    assert!(run(&baseline));
    let peak = baseline.peak_query_memory_bytes();

    // Act
    let observed = [(peak, true), (peak - 1, false)].map(|(budget, succeeds)| {
        let controls = controls(budget);
        let matches = run(&controls) == succeeds;
        (matches, controls.current_query_memory_bytes())
    });

    // Assert
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    assert_eq!(observed, [(true, 0), (true, 0)]);
}
