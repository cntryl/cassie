//! Deferred probes for native array JSONB comparison.
use super::{compare_arrays, compare_arrays_controlled};
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::types::{ArrayDimension, SqlArray, Value};
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
fn json(text: &str) -> Value {
    Value::Json(serde_json::from_str(text).expect("JSON"))
}
fn array(values: Vec<Value>, lower: i32) -> SqlArray {
    let length = i32::try_from(values.len()).expect("length");
    SqlArray::new(
        vec![ArrayDimension {
            length,
            lower_bound: lower,
        }],
        values,
    )
    .expect("array")
}

#[test]
fn should_match_native_array_element_null_and_dimension_order() {
    // Arrange
    let cases = [
        (array(vec![json("1.0")], 1), array(vec![json("1")], 1)),
        (array(vec![json("[]")], 1), array(vec![json("null")], 1)),
        (array(vec![Value::Null], 1), array(vec![json("null")], 1)),
        (
            array(vec![json(r#"{"z":0}"#)], 1),
            array(vec![json(r#"{"aa":0}"#)], 1),
        ),
        (array(vec![json("1")], -2), array(vec![json("1")], 1)),
        (
            array(vec![json("1"), json("2")], 1),
            array(vec![json("2"), json("1")], 1),
        ),
        (
            array(vec![Value::Int64(1)], 1),
            array(vec![Value::Int64(2)], 1),
        ),
    ];
    let controls = controls(20 * 1024 * 1024);
    // Act / Assert
    for (left, right) in cases {
        assert_eq!(
            compare_arrays_controlled(&left, &right, &controls).expect("comparison"),
            compare_arrays(&left, &right)
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_both_serialized_values_and_arenas_after_real_comparison_cancellation() {
    // Arrange
    let text = format!("\"{}\"", "x".repeat(2_048));
    let left = array(vec![json(&text)], 1);
    let right = array(vec![json(&text)], 1);
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    crate::runtime::accounted::json_arena::cancel_comparison_work_once(cancel);
    // Act
    let result = compare_arrays_controlled(&left, &right, &controls);
    // Assert
    assert!(result.is_err());
    assert!(controls.peak_query_memory_bytes() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_the_measured_array_comparison_overlap() {
    // Arrange
    let left = array(vec![json(r#"{"a":[1,2]}"#)], 1);
    let right = array(vec![json(r#"{"a":[1,3]}"#)], 1);
    let baseline = controls(10 * 1024 * 1024);
    assert!(compare_arrays_controlled(&left, &right, &baseline)
        .expect("baseline")
        .is_some());
    let peak = baseline.peak_query_memory_bytes();
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = controls(budget);
        assert_eq!(
            compare_arrays_controlled(&left, &right, &controls).is_ok(),
            succeeds
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_propagate_comparison_cancellation_through_binary_and_between() {
    // Arrange
    use crate::executor::batch::BatchRow;
    use crate::executor::filter::{evaluate_expr_value_with_context, EvalContext};
    use crate::types::DataType;
    use std::collections::HashMap;
    let text = format!("\"{}\"", "x".repeat(2_048));
    let left = array(vec![json(&text)], 1);
    let right = array(vec![json(&text)], 1);
    let mut row = BatchRow::new(vec![
        ("a".into(), Value::Array(left)),
        ("b".into(), Value::Array(right)),
    ]);
    row.set_data_types(
        vec![
            DataType::Array(Box::new(DataType::Jsonb)),
            DataType::Array(Box::new(DataType::Jsonb)),
        ]
        .into(),
    );
    let functions = HashMap::new();
    for sql in ["a = b", "a BETWEEN b AND b"] {
        let expression = crate::sql::parser::parse_expression(sql).expect("expression");
        let cancel = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancel.clone(),
        );
        crate::runtime::accounted::json_arena::cancel_comparison_work_once(cancel);
        let context = EvalContext {
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
        // Act
        let result = evaluate_expr_value_with_context(&row, &expression, context);
        // Assert
        assert!(result.is_err(), "{sql}");
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_left_owner_after_actual_right_serialization_cancellation() {
    // Arrange
    let text = format!("\"{}\"", "x".repeat(2_048));
    let left = array(vec![json(&text)], 1);
    let right = array(vec![json(&text)], 1);
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    super::jsonb_array_comparison::cancel_right_copy_once(cancel);
    // Act
    let result = compare_arrays_controlled(&left, &right, &controls);
    // Assert
    assert!(result.is_err());
    assert!(controls.peak_query_memory_bytes() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_borrow_evaluated_jsonb_arrays_for_binary_and_between() {
    // Arrange
    use crate::executor::batch::BatchRow;
    use crate::executor::filter::{evaluate_expr_value_with_context, EvalContext};
    use crate::types::DataType;
    use std::collections::HashMap;
    let mut row = BatchRow::new(vec![
        ("a".into(), Value::Array(array(vec![json("1")], 1))),
        ("b".into(), Value::Array(array(vec![json("1.0")], 1))),
    ]);
    row.set_data_types(
        vec![
            DataType::Array(Box::new(DataType::Jsonb)),
            DataType::Array(Box::new(DataType::Jsonb)),
        ]
        .into(),
    );
    let controls = controls(10 * 1024 * 1024);
    let functions = HashMap::new();
    for sql in ["a = b", "a BETWEEN b AND b"] {
        let expression = crate::sql::parser::parse_expression(sql).expect("expression");
        let context = EvalContext {
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
        crate::executor::filter::take_array_value_clone_count();
        // Act
        let result =
            evaluate_expr_value_with_context(&row, &expression, context).expect("comparison");
        // Assert
        assert!(matches!(result, Value::Bool(true)), "{sql}");
        assert_eq!(
            crate::executor::filter::take_array_value_clone_count(),
            0,
            "{sql}"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_parameter_null_and_non_json_array_fallback() {
    // Arrange
    use crate::executor::batch::BatchRow;
    use crate::executor::filter::{evaluate_expr_value_with_context, EvalContext};
    use crate::types::DataType;
    use std::collections::HashMap;
    let json_array = array(vec![json("1")], 1);
    let mut row = BatchRow::new(vec![
        ("a".into(), Value::Array(json_array.clone())),
        ("n".into(), Value::Null),
        ("x".into(), Value::Array(array(vec![Value::Int64(1)], 1))),
    ]);
    row.set_data_types(
        vec![
            DataType::Array(Box::new(DataType::Jsonb)),
            DataType::Array(Box::new(DataType::Jsonb)),
            DataType::Array(Box::new(DataType::BigInt)),
        ]
        .into(),
    );
    let params = [Value::Array(json_array)];
    let functions = HashMap::new();
    let controls = controls(10 * 1024 * 1024);
    let context = EvalContext {
        params: &params,
        search_context: None,
        user_functions: &functions,
        local_args: None,
        session: None,
        exists: None,
        controls: Some(&controls),
        memory: None,
        deletion_memory: None,
    };
    let parameter = crate::sql::parser::parse_expression("a = $1").expect("parameter");
    let null = crate::sql::parser::parse_expression("a = n").expect("null");
    let mixed = crate::sql::parser::parse_expression("x = x").expect("mixed");
    // Act / Assert
    crate::executor::filter::take_array_value_clone_count();
    assert!(matches!(
        evaluate_expr_value_with_context(&row, &parameter, context),
        Ok(Value::Bool(true))
    ));
    assert_eq!(crate::executor::filter::take_array_value_clone_count(), 0);
    assert!(matches!(
        evaluate_expr_value_with_context(&row, &null, context),
        Ok(Value::Null)
    ));
    crate::executor::filter::take_array_value_clone_count();
    assert!(matches!(
        evaluate_expr_value_with_context(&row, &mixed, context),
        Ok(Value::Bool(true))
    ));
    assert!(crate::executor::filter::take_array_value_clone_count() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_borrow_qualified_jsonb_columns_and_preserve_ownerless_path() {
    // Arrange
    use crate::executor::batch::BatchRow;
    use crate::executor::filter::{evaluate_expr_value_with_context, EvalContext};
    use crate::types::DataType;
    use std::collections::HashMap;
    let mut row = BatchRow::new(vec![
        ("t.a".into(), Value::Array(array(vec![json("1")], 1))),
        ("t.b".into(), Value::Array(array(vec![json("1")], 1))),
    ]);
    row.set_data_types(
        vec![
            DataType::Array(Box::new(DataType::Jsonb)),
            DataType::Array(Box::new(DataType::Jsonb)),
        ]
        .into(),
    );
    let expression = crate::sql::parser::parse_expression("t.a = t.b").expect("expression");
    let controls = controls(10 * 1024 * 1024);
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
        deletion_memory: None,
    };
    // Act / Assert
    crate::executor::filter::take_array_value_clone_count();
    assert!(matches!(
        evaluate_expr_value_with_context(&row, &expression, context),
        Ok(Value::Bool(true))
    ));
    assert_eq!(crate::executor::filter::take_array_value_clone_count(), 0);
    assert!(matches!(
        evaluate_expr_value_with_context(
            &row,
            &expression,
            EvalContext {
                controls: None,
                ..context
            }
        ),
        Ok(Value::Bool(true))
    ));
    assert!(crate::executor::filter::take_array_value_clone_count() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_evaluate_between_high_operand_before_admitting_low_comparison() {
    // Arrange
    use crate::executor::batch::BatchRow;
    use crate::executor::filter::{evaluate_expr_value_with_context, EvalContext};
    use crate::types::DataType;
    use std::collections::HashMap;
    let mut row = BatchRow::new(vec![
        ("a".into(), Value::Array(array(vec![json("1")], 1))),
        ("b".into(), Value::Array(array(vec![json("1")], 1))),
    ]);
    row.set_data_types(
        vec![
            DataType::Array(Box::new(DataType::Jsonb)),
            DataType::Array(Box::new(DataType::Jsonb)),
        ]
        .into(),
    );
    let expression =
        crate::sql::parser::parse_expression("a BETWEEN b AND (1 / 0)").expect("expression");
    let controls = controls(0);
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
        deletion_memory: None,
    };
    // Act
    let error = evaluate_expr_value_with_context(&row, &expression, context)
        .expect_err("division fails first");
    // Assert
    assert!(error.to_string().contains("division by zero"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
