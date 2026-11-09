use super::{from_value, ScalarValue};
use crate::config::CassieRuntimeLimits;
use crate::executor::filter::{EvalContext, EvaluationMemory};
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::types::Value;
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

#[test]
fn should_match_compact_json_value_spelling_and_hold_its_owner() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let values = [
        serde_json::json!({"z":"é\n", "a":[1,true,null]}),
        serde_json::json!({"$serde_json::private::Number":"7"}),
        serde_json::json!("quote\"\\\0"),
    ];
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
    // Act / Assert
    for json in values {
        let expected = json.to_string();
        let actual = from_value(&Value::Json(json), context).expect("value");
        assert!(matches!(actual, ScalarValue::Json(text) if text == expected));
    }
    assert!(controls.current_query_memory_bytes() > 0);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_general_owner_and_ownerless_fallbacks_unchanged() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    let general = EvaluationMemory::new(&controls).expect("general");
    let deletion = EvaluationMemory::new(&controls).expect("deletion");
    let functions = HashMap::new();
    let value = Value::Json(serde_json::json!({"a":1}));
    let base = EvalContext {
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
        base,
        EvalContext {
            memory: None,
            deletion_memory: None,
            ..base
        },
        EvalContext {
            memory: None,
            controls: None,
            ..base
        },
    ] {
        assert!(
            matches!(from_value(&value, context), Ok(ScalarValue::Json(text)) if text == r#"{"a":1}"#)
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_retain_compact_json_text_with_a_shared_general_and_deletion_owner() {
    // Arrange
    let foreign = controls(10 * 1024 * 1024);
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
        memory: Some(&owner),
        deletion_memory: Some(&owner),
    };
    let values = [
        serde_json::json!({"z":"é\n", "a":[1,true,null]}),
        serde_json::json!({"$serde_json::private::Number":"7"}),
        serde_json::json!("quote\"\\\0"),
    ];

    // Act
    let rejected = from_value(
        &Value::Json(values[0].clone()),
        EvalContext {
            controls: Some(&foreign),
            ..context
        },
    );
    let outputs = values.map(|json| {
        let expected = json.to_string();
        let output = from_value(&Value::Json(json), context).expect("shared owner");
        (output, expected)
    });
    let retained = controls.current_query_memory_bytes();

    // Assert
    assert!(rejected.is_err());
    assert_eq!(foreign.current_query_memory_bytes(), 0);
    for (output, expected) in &outputs {
        assert!(matches!(output, ScalarValue::Json(text) if text == expected));
    }
    assert!(retained > 0);
    drop(outputs);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_shared_owner_after_actual_compact_copy_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let functions = HashMap::new();
    let value = Value::Json(serde_json::json!("é".repeat(2048)));
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
    super::cancel_copy_once(cancel.clone(), 3);

    // Act
    let result = from_value(&value, context);

    // Assert
    assert!(cancel.is_cancelled());
    assert!(result.is_err());
    drop(result);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_shared_leaf_output_while_general_scratch_is_retained() {
    // Arrange
    let value = Value::Json(serde_json::json!(["é".repeat(1024), {"n": 1}]));
    let functions = HashMap::new();
    let baseline = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&baseline).expect("owner");
    owner.grow(512).expect("general scratch");
    let context = EvalContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        local_args: None,
        session: None,
        exists: None,
        controls: Some(&baseline),
        memory: Some(&owner),
        deletion_memory: Some(&owner),
    };
    let output = from_value(&value, context).expect("baseline");
    let peak = baseline.peak_query_memory_bytes();
    drop(output);
    drop(owner);

    // Act
    let cases = [(peak, true), (peak - 1, false)].map(|(budget, succeeds)| {
        let controls = controls(budget);
        let owner = EvaluationMemory::new(&controls).expect("owner");
        owner.grow(512).expect("general scratch");
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
        let result = from_value(&value, context);
        let matches = result.is_ok() == succeeds;
        drop(result);
        drop(owner);
        (matches, controls.current_query_memory_bytes())
    });

    // Assert
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    assert_eq!(cases, [(true, 0), (true, 0)]);
}

#[test]
fn should_release_partial_leaf_after_actual_copy_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let functions = HashMap::new();
    let value = Value::Json(serde_json::json!("é".repeat(2_048)));
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
    super::cancel_copy_once(cancel, 3);
    // Act
    let result = from_value(&value, context);
    // Assert
    assert!(result.is_err());
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_foreign_tracker_and_admit_measured_leaf_peak() {
    // Arrange
    let baseline = controls(10 * 1024 * 1024);
    let foreign = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&baseline).expect("owner");
    let functions = HashMap::new();
    let value = Value::Json(serde_json::json!(["x".repeat(1_024), {"k": 1}]));
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
    assert!(from_value(
        &value,
        EvalContext {
            controls: Some(&foreign),
            ..context
        }
    )
    .is_err());
    assert_eq!(foreign.current_query_memory_bytes(), 0);
    let actual = from_value(&value, context).expect("baseline");
    let peak = baseline.peak_query_memory_bytes();
    drop(actual);
    drop(owner);
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
        let actual = from_value(&value, context);
        assert_eq!(actual.is_ok(), succeeds);
        drop(actual);
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_retain_column_and_parameter_serializations_through_extraction() {
    // Arrange
    use crate::executor::batch::BatchRow;
    use crate::executor::filter::evaluate_expr_value_with_context;
    use crate::types::DataType;
    let controls = controls(10 * 1024 * 1024);
    let functions = HashMap::new();
    let value = Value::Json(serde_json::json!({"a":"payload"}));
    let params = [value.clone()];
    let mut row = BatchRow::new(vec![("doc".into(), value)]);
    row.set_data_types(vec![DataType::Jsonb].into());
    // Act / Assert
    for sql in ["doc ->> 'a'", "$1 ->> 'a'"] {
        let owner = EvaluationMemory::new(&controls).expect("owner");
        let context = EvalContext {
            params: &params,
            search_context: None,
            user_functions: &functions,
            local_args: None,
            session: None,
            exists: None,
            controls: Some(&controls),
            memory: None,
            deletion_memory: Some(&owner),
        };
        let expression = crate::sql::parser::parse_expression(sql).expect("expression");
        let result =
            evaluate_expr_value_with_context(&row, &expression, context).expect("extraction");
        assert!(matches!(&result, Value::String(text) if text == "payload"));
        assert!(controls.current_query_memory_bytes() > 0);
        drop(result);
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_retain_source_lease_until_owner_drops_after_sibling_error() {
    // Arrange
    use crate::executor::batch::BatchRow;
    use crate::executor::filter::evaluate_expr_value_with_context;
    use crate::types::DataType;
    let controls = controls(10 * 1024 * 1024);
    let functions = HashMap::new();
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let mut row = BatchRow::new(vec![
        ("a".into(), Value::Json(serde_json::json!({"a":1}))),
        ("b".into(), Value::Float64(f64::NAN)),
    ]);
    row.set_data_types(vec![DataType::Jsonb, DataType::Jsonb].into());
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
    let expression = crate::sql::parser::parse_expression("a || b").expect("expression");
    // Act
    let result = evaluate_expr_value_with_context(&row, &expression, context);
    // Assert
    assert!(result.is_err());
    assert!(controls.current_query_memory_bytes() > 0);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
