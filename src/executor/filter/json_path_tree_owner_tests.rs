use super::{evaluate, Value};
use crate::config::CassieRuntimeLimits;
use crate::executor::filter::{EvalContext, EvaluationMemory};
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::{collections::HashMap, time::Instant};

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
fn arguments() -> [Value; 2] {
    [
        Value::String(r#"{"a":[1,2]}"#.into()),
        Value::String("$.a[*]".into()),
    ]
}

#[test]
fn should_retain_existing_jsonpath_tree_lease_for_array_and_first_variants() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&controls).expect("owner");
    // Act / Assert
    for name in [
        "jsonb_path_query_array",
        "jsonb_path_query_first",
        "jsonb_path_query_array_tz",
        "jsonb_path_query_first_tz",
    ] {
        let expected = evaluate(name, &arguments(), Some(&controls), None)
            .expect("selected")
            .expect("legacy");
        let result = evaluate(name, &arguments(), Some(&controls), Some(&owner))
            .expect("selected")
            .expect("result");
        assert_eq!(result, expected);
        assert!(matches!(result, Value::Json(_)));
        assert!(owner.bytes() > 0);
    }
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_hold_tree_lease_while_admitting_its_compact_scalar_output() {
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
    let result = evaluate(
        "jsonb_path_query_array",
        &arguments(),
        Some(&controls),
        Some(&owner),
    )
    .expect("selected")
    .expect("result");
    let before = owner.bytes();
    // Act
    let scalar =
        crate::executor::filter::json_scalar_leaf::from_value(&result, context).expect("scalar");
    // Assert
    assert!(owner.bytes() > before);
    assert!(matches!(
        &scalar,
        crate::executor::filter::ScalarValue::Json(_)
    ));
    drop(scalar);
    drop(result);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_not_transfer_for_missing_null_boolean_or_ownerless_results() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let missing = [
        Value::String("{}".into()),
        Value::String("$.missing".into()),
    ];
    let null = [Value::Null, Value::String("$.a".into())];
    // Act
    let a = evaluate(
        "jsonb_path_query_first",
        &missing,
        Some(&controls),
        Some(&owner),
    )
    .expect("selected")
    .expect("result");
    let b = evaluate(
        "jsonb_path_query_array",
        &null,
        Some(&controls),
        Some(&owner),
    )
    .expect("selected")
    .expect("result");
    let c = evaluate(
        "jsonb_path_exists",
        &arguments(),
        Some(&controls),
        Some(&owner),
    )
    .expect("selected")
    .expect("result");
    let d = evaluate(
        "jsonb_path_query_array",
        &arguments(),
        Some(&controls),
        None,
    )
    .expect("selected")
    .expect("result");
    let invalid = evaluate(
        "jsonb_path_query_array",
        &[Value::String("{}".into()), Value::String("$[".into())],
        Some(&controls),
        Some(&owner),
    )
    .expect("selected");
    // Assert
    assert!(matches!(a, Value::Null));
    assert!(matches!(b, Value::Null));
    assert!(matches!(c, Value::Bool(_)));
    assert!(matches!(d, Value::Json(_)));
    assert!(invalid.is_err());
    assert_eq!(owner.bytes(), 0);
    drop(d);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_tree_after_post_result_cancel_and_reject_foreign_owner() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let run_controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let owner = EvaluationMemory::new(&run_controls).expect("owner");
    super::super::json_result_owner::cancel_after_output_once(cancel);
    // Act
    let result = evaluate(
        "jsonb_path_query_array",
        &arguments(),
        Some(&run_controls),
        Some(&owner),
    )
    .expect("selected");
    // Assert
    assert!(result.is_err());
    drop(owner);
    assert_eq!(run_controls.current_query_memory_bytes(), 0);
    let owner_controls = controls(10 * 1024 * 1024);
    let foreign = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&owner_controls).expect("owner");
    let result = evaluate(
        "jsonb_path_query_array",
        &arguments(),
        Some(&foreign),
        Some(&owner),
    )
    .expect("selected");
    assert!(result.is_err());
    assert_eq!(foreign.current_query_memory_bytes(), 0);
    drop(owner);
    assert_eq!(owner_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_measured_jsonpath_tree_workspace_peak() {
    // Arrange
    let baseline = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&baseline).expect("owner");
    let result = evaluate(
        "jsonb_path_query_array",
        &arguments(),
        Some(&baseline),
        Some(&owner),
    )
    .expect("selected")
    .expect("result");
    let peak = baseline.peak_query_memory_bytes();
    drop(result);
    drop(owner);
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = controls(budget);
        let owner = EvaluationMemory::new(&controls).expect("owner");
        let result = evaluate(
            "jsonb_path_query_array",
            &arguments(),
            Some(&controls),
            Some(&owner),
        )
        .expect("selected");
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_keep_general_owner_jsonpath_route_on_its_existing_path() {
    // Arrange
    use crate::executor::batch::BatchRow;
    use crate::executor::filter::evaluate_expr_value_with_context;
    let controls = controls(10 * 1024 * 1024);
    let general = EvaluationMemory::new(&controls).expect("general");
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let functions = HashMap::new();
    let row = BatchRow::new(vec![]);
    let context = EvalContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        local_args: None,
        session: None,
        exists: None,
        controls: Some(&controls),
        memory: Some(&general),
        deletion_memory: Some(&owner),
    };
    let expression = crate::sql::parser::parse_expression(
        "jsonb_path_query_array('{\"a\":[1]}'::JSONB, '$.a[*]')",
    )
    .expect("expression");
    // Act
    let result = evaluate_expr_value_with_context(&row, &expression, context).expect("result");
    // Assert
    assert!(matches!(&result, Value::Json(_)));
    assert_eq!(owner.bytes(), 0);
    drop(result);
    drop(general);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_grow_existing_tree_lease_to_the_retained_estimate_or_release_it() {
    // Arrange
    let mut text = String::with_capacity(65_536);
    text.push('x');
    let json = serde_json::Value::String(text);
    let expected = crate::runtime::accounted::json::retained_bytes(&json).expect("estimate");
    let probe = controls(10 * 1024 * 1024);
    let _ =
        crate::runtime::accounted::json::tree_estimate::estimate(&json, &probe).expect("scratch");
    let scratch = probe.peak_query_memory_bytes();
    let budget = scratch + 1;
    assert!(budget < expected);
    // Act / Assert
    for (limit, succeeds) in [(10 * 1024 * 1024, true), (budget, false)] {
        let controls = controls(limit);
        let owner = EvaluationMemory::new(&controls).expect("owner");
        let memory = controls.reserve_query_memory(1).expect("workspace");
        let mut case_text = String::with_capacity(65_536);
        case_text.push('x');
        let case_json = serde_json::Value::String(case_text);
        let case_estimate =
            crate::runtime::accounted::json::retained_bytes(&case_json).expect("estimate");
        let result = super::super::json_result_owner::retain_json_tree(
            Value::Json(case_json),
            Some(memory),
            Some(&owner),
            Some(&controls),
        );
        assert_eq!(result.is_ok(), succeeds);
        if succeeds {
            assert!(owner.bytes() >= case_estimate);
        }
        assert!(controls.peak_query_memory_bytes() > 1);
        drop(result);
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_the_tree_and_workspace_after_actual_estimator_move_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let memory = controls.reserve_query_memory(512).expect("workspace");
    let mut json = serde_json::Value::Null;
    for _ in 0..16 {
        json = serde_json::Value::Array(vec![json]);
    }
    crate::runtime::accounted::json::tree_estimate::cancel_move_once(cancel);
    // Act
    let result = super::super::json_result_owner::retain_json_tree(
        Value::Json(json),
        Some(memory),
        Some(&owner),
        Some(&controls),
    );
    // Assert
    assert!(result.is_err());
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
