//! Deferred object-key Value and reservation handoff probes.
use super::{evaluate, evaluate_with_owner, Value};
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
fn run(
    name: &str,
    input: Value,
    controls: Option<&QueryExecutionControls>,
    owner: Option<&EvaluationMemory>,
) -> Result<Value, String> {
    evaluate_with_owner(name, &[input], controls, owner)
        .expect("known function")
        .map_err(|error| error.to_string())
}

#[test]
fn should_match_legacy_object_key_order_duplicates_sentinels_and_errors() {
    // Arrange
    let inputs = [
        Value::String(r#"{"z":1,"aa":2,"a":3,"a":4,"é":5}"#.into()),
        Value::String(r#"{"\u0061":1,"a":2,"\u0000":3}"#.into()),
        Value::String(r#"{"$serde_json::private::Number":"7"}"#.into()),
        Value::String("[1]".into()),
        Value::String("[}".into()),
        Value::Null,
        Value::Json(serde_json::json!({"z":1,"a":2})),
    ];
    // Act / Assert
    for name in ["json_object_keys", "jsonb_object_keys"] {
        for input in &inputs {
            let controls = controls(16 * 1024 * 1024);
            let owner = EvaluationMemory::new(&controls).expect("owner");
            let expected = evaluate(name, &[input.clone()], Some(&controls))
                .expect("known")
                .map_err(|error| error.to_string());
            let actual = run(name, input.clone(), Some(&controls), Some(&owner));
            assert_eq!(actual, expected, "{name} {input:?}");
            drop(actual);
            drop(expected);
            drop(owner);
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }
}

#[test]
fn should_release_object_key_output_on_actual_copy_and_traversal_cancellation() {
    // Arrange
    for copy in [true, false] {
        let cancel = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancel.clone(),
        );
        let owner = EvaluationMemory::new(&controls).expect("owner");
        if copy {
            crate::runtime::accounted::json_arena::cancel_object_keys_copy_once(cancel.clone());
        } else {
            crate::runtime::accounted::json_arena::cancel_object_keys_visit_once(cancel.clone());
        }
        // Act
        let result = run(
            "json_object_keys",
            Value::String(r#"{"long-key":1,"b":2}"#.into()),
            Some(&controls),
            Some(&owner),
        );
        // Assert
        assert!(cancel.is_cancelled());
        assert!(result.is_err());
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_enforce_object_key_scratch_arena_and_output_overlap() {
    // Arrange
    let input = Value::String(r#"{"first":1,"second":2,"third":3}"#.into());
    let baseline = controls(16 * 1024 * 1024);
    let owner = EvaluationMemory::new(&baseline).expect("owner");
    let value = run(
        "jsonb_object_keys",
        input.clone(),
        Some(&baseline),
        Some(&owner),
    )
    .expect("result");
    let peak = baseline.peak_query_memory_bytes();
    assert!(owner.bytes() > 0);
    drop(value);
    drop(owner);
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = controls(budget);
        let owner = EvaluationMemory::new(&controls).expect("owner");
        let result = run(
            "jsonb_object_keys",
            input.clone(),
            Some(&controls),
            Some(&owner),
        );
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_reject_foreign_object_key_owner_without_retaining_allocations() {
    // Arrange
    let controls = controls(16 * 1024 * 1024);
    let foreign = self::controls(16 * 1024 * 1024);
    let owner = EvaluationMemory::new(&foreign).expect("owner");
    // Act
    let result = run(
        "json_object_keys",
        Value::String(r#"{"a":1}"#.into()),
        Some(&controls),
        Some(&owner),
    );
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(owner);
    assert_eq!(foreign.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_general_owner_and_ownerless_calls_on_legacy_construction() {
    // Arrange
    use crate::executor::batch::BatchRow;
    use crate::executor::filter::{evaluate_expr_value_with_context, EvalContext};
    use std::collections::HashMap;
    let controls = controls(16 * 1024 * 1024);
    let general = EvaluationMemory::new(&controls).expect("general");
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let input = Value::String(r#"{"a":1}"#.into());
    let row = BatchRow::new(vec![]);
    let expression =
        crate::sql::parser::parse_expression("json_object_keys('{\"a\":1}')").expect("expression");
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
        deletion_memory: Some(&owner),
    };
    // Act
    let general_result =
        evaluate_expr_value_with_context(&row, &expression, context).expect("general");
    let ownerless_result = run("json_object_keys", input, None, Some(&owner)).expect("ownerless");
    // Assert
    assert_eq!(general_result, ownerless_result);
    assert_eq!(owner.bytes(), 0);
    drop(general_result);
    drop(ownerless_result);
    drop(general);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
