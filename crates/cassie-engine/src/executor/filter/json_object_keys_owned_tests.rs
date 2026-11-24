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
fn should_match_legacy_object_key_order_duplicates_sentinels_with_errors() {
    // Arrange
    let inputs = [
        Value::String(r#"{"z":1,"aa":2,"a":3,"a":4,"é":5}"#.into()),
        Value::String(r#"{"\u0061":1,"a":2,"\u0000":3}"#.into()),
        Value::String(r#"{"$serde_json::private::Number":"7"}"#.into()),
        Value::String("[1]".into()),
        Value::String("[\u{7d}".into()),
        Value::Null,
        Value::Json(serde_json::json!({"z":1,"a":2})),
    ];
    // Act
    for name in ["json_object_keys", "jsonb_object_keys"] {
        for input in &inputs {
            let controls = controls(16 * 1024 * 1024);
            let owner = EvaluationMemory::new(&controls).expect("owner");
            let expected = evaluate(name, std::slice::from_ref(input), Some(&controls))
                .expect("known")
                .map_err(|error| error.to_string());
            let actual = run(name, input.clone(), Some(&controls), Some(&owner));
            // Assert
            assert_eq!(actual, expected, "{name} {input:?}");
            drop(actual);
            drop(expected);
            drop(owner);
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }
}

#[test]
fn should_release_object_key_output_on_actual_copy_with_traversal_cancellation() {
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
        assert!(cancel.is_canceled());
        assert!(result.is_err());
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_enforce_object_key_scratch_arena_with_output_overlap() {
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
    // Assert
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
fn should_leave_general_owner_with_ownerless_calls_on_legacy_construction() {
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

#[test]
fn should_match_value_backed_object_keys_across_unicode_prefixes_with_sentinels() {
    // Arrange
    let mut map = serde_json::Map::new();
    for key in ["é", "aa", "a", "", "\0", "$serde_json::private::Number"] {
        map.insert(key.into(), serde_json::json!(1));
    }
    let inputs = [
        Value::Json(serde_json::Value::Object(map)),
        Value::Json(serde_json::Value::Array(vec![])),
        Value::Null,
    ];
    // Act / Assert
    for name in ["json_object_keys", "jsonb_object_keys"] {
        for input in &inputs {
            let controls = controls(16 * 1024 * 1024);
            let owner = EvaluationMemory::new(&controls).expect("owner");
            let expected = evaluate(name, std::slice::from_ref(input), Some(&controls))
                .expect("known")
                .map_err(|error| error.to_string());
            let actual = run(name, input.clone(), Some(&controls), Some(&owner));
            // Assert
            assert_eq!(actual, expected, "{name} {input:?}");
            if matches!(input, Value::Json(serde_json::Value::Object(_))) {
                assert!(owner.bytes() > 0);
            }
            drop(actual);
            drop(expected);
            drop(owner);
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }
}

#[test]
fn should_release_value_key_sort_with_output_after_actual_work_cancellation() {
    // Arrange
    for stage in 0..3 {
        let cancel = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancel.clone(),
        );
        let owner = EvaluationMemory::new(&controls).expect("owner");
        match stage {
            0 => crate::runtime::accounted::json_arena::cancel_object_keys_sort_compare_once(
                cancel.clone(),
            ),
            1 => crate::runtime::accounted::json_arena::cancel_object_keys_sort_move_once(
                cancel.clone(),
            ),
            _ => {
                crate::runtime::accounted::json_arena::cancel_object_keys_copy_once(cancel.clone());
            }
        }
        let input = Value::Json(serde_json::json!({"long-a":1,"long-b":2,"long-c":3}));
        // Act
        let result = run("json_object_keys", input, Some(&controls), Some(&owner));
        // Assert
        assert!(cancel.is_canceled());
        assert!(result.is_err());
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_enforce_value_key_sort_source_with_output_overlap() {
    // Arrange
    let input = Value::Json(serde_json::json!({"first":1,"second":2,"third":3}));
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
    // Assert
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
fn should_release_value_key_output_on_foreign_owner() {
    // Arrange
    let controls = controls(16 * 1024 * 1024);
    let foreign = self::controls(16 * 1024 * 1024);
    let owner = EvaluationMemory::new(&foreign).expect("owner");
    // Act
    let result = run(
        "json_object_keys",
        Value::Json(serde_json::json!({"a":1})),
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
fn should_count_selected_object_input_exactly_like_legacy_serde_writer() {
    // Arrange
    let cases = [
        serde_json::json!({}),
        serde_json::json!({"a":[1,true,null,{"x":"é\\n\\\""}]}),
        serde_json::from_str(r#"{"n":123456789012345678901234567890.001,"s":"\u0000"}"#)
            .expect("JSON"),
        serde_json::json!({"$serde_json::private::Number":"7"}),
    ];
    let controls = controls(16 * 1024 * 1024);
    // Act / Assert
    for value in cases {
        let expected =
            crate::executor::retained_memory::serialized_json_bytes(&value).expect("legacy count");
        let actual = super::selected_object_key_size(&value, &controls).expect("checked count");
        // Assert
        assert_eq!(actual, expected);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_count_stack_after_actual_selected_preflight_scan_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let owner = EvaluationMemory::new(&controls).expect("owner");
    super::cancel_object_key_count_scan_once(cancel.clone());
    let input = Value::Json(serde_json::json!({"a":"x".repeat(2_048)}));
    // Act
    let result = run("json_object_keys", input, Some(&controls), Some(&owner));
    // Assert
    assert!(cancel.is_canceled());
    assert!(result.is_err());
    assert_eq!(owner.bytes(), 0);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_ownerless_value_preflight_on_legacy_route() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let input = Value::Json(serde_json::json!({"a":"x".repeat(2_048)}));
    super::cancel_object_key_count_scan_once(cancel.clone());
    // Act
    let result = run("json_object_keys", input.clone(), Some(&controls), None);
    // Assert
    assert!(result.is_ok());
    assert!(!cancel.is_canceled());
    let owner = EvaluationMemory::new(&controls).expect("owner");
    assert!(run("json_object_keys", input, Some(&controls), Some(&owner)).is_err());
    assert!(cancel.is_canceled());
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_nonobject_error_before_selected_count_hook() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let owner = EvaluationMemory::new(&controls).expect("owner");
    super::cancel_object_key_count_scan_once(cancel.clone());
    let array = Value::Json(serde_json::json!(["x".repeat(2_048)]));
    // Act
    let result = run("json_object_keys", array, Some(&controls), Some(&owner));
    // Assert
    assert!(result
        .expect_err("non-object")
        .contains("cannot call json_object_keys on a non-object"));
    assert!(!cancel.is_canceled());
    let object = Value::Json(serde_json::json!({"a":"x".repeat(2_048)}));
    assert!(run("json_object_keys", object, Some(&controls), Some(&owner)).is_err());
    assert!(cancel.is_canceled());
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
