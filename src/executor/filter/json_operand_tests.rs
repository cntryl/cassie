use super::OwnedOperand;
use crate::config::CassieRuntimeLimits;
use crate::executor::filter::{json_concat::as_json_document, ScalarValue};
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
fn should_match_legacy_scalar_conversion_for_escapes_and_float_boundaries() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    let values = [
        ScalarValue::Null,
        ScalarValue::Bool(true),
        ScalarValue::Int(i64::MIN),
        ScalarValue::Int(i64::MAX),
        ScalarValue::Float(-0.0),
        ScalarValue::Float(f64::MIN_POSITIVE),
        ScalarValue::Float(f64::from_bits(1)),
        ScalarValue::Float(f64::MAX),
        ScalarValue::Float(1e-7),
        ScalarValue::Float(1e21),
        ScalarValue::Float(f64::NAN),
        ScalarValue::Float(f64::INFINITY),
        ScalarValue::Str("\0\u{1}\n\t\"\\é".into()),
        ScalarValue::Json("{}".into()),
        ScalarValue::Array(crate::types::SqlArray::from_elements(vec![]).expect("array")),
    ];
    // Act / Assert
    for value in values {
        let expected = as_json_document(&value).map_err(|error| error.to_string());
        let actual = OwnedOperand::convert(value, &controls).map_err(|error| error.to_string());
        match (expected, actual) {
            (Ok(ScalarValue::Json(a)), Ok(actual)) => {
                assert!(matches!(actual.value(), ScalarValue::Json(b) if &a == b))
            }
            (Ok(ScalarValue::Null), Ok(actual)) => {
                assert!(matches!(actual.value(), ScalarValue::Null))
            }
            (Err(a), Err(b)) => assert_eq!(a, b),
            _ => panic!("scalar conversion parity"),
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_partial_operand_after_actual_string_copy_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let value = ScalarValue::Str("é".repeat(2_048));
    super::cancel_copy_once(cancellation, 3);
    // Act
    let result = OwnedOperand::convert(value, &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_left_conversion_when_the_right_conversion_fails() {
    // Arrange
    use crate::executor::batch::BatchRow;
    use crate::executor::filter::{
        evaluate_expr_value_with_context, EvalContext, EvaluationMemory,
    };
    use crate::types::{DataType, Value};
    use std::collections::HashMap;
    let controls = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let mut row = BatchRow::new(vec![
        ("a".into(), Value::String("payload".repeat(128))),
        ("b".into(), Value::Float64(f64::NAN)),
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
    let result = evaluate_expr_value_with_context(&row, &expression, context);
    // Assert
    assert!(result.is_err());
    assert!(controls.peak_query_memory_bytes() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_converted_operands_when_final_owner_is_foreign() {
    // Arrange
    use crate::executor::batch::BatchRow;
    use crate::executor::filter::{
        evaluate_expr_value_with_context, EvalContext, EvaluationMemory,
    };
    use crate::types::{DataType, Value};
    use std::collections::HashMap;
    let owner_controls = controls(10 * 1024 * 1024);
    let run_controls = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&owner_controls).expect("owner");
    let mut row = BatchRow::new(vec![
        ("a".into(), Value::String("payload".into())),
        ("b".into(), Value::String("second".into())),
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
        controls: Some(&run_controls),
        memory: None,
        deletion_memory: Some(&owner),
    };
    // Act
    let result = evaluate_expr_value_with_context(&row, &expression, context);
    // Assert
    assert!(matches!(
        result,
        Err(crate::executor::QueryError::Cassie(
            crate::app::CassieError::ResourceLimit(message)
        )) if message == "JSON deletion owner uses a different query tracker"
    ));
    assert!(run_controls.peak_query_memory_bytes() > 0);
    assert_eq!(run_controls.current_query_memory_bytes(), 0);
    drop(owner);
    assert_eq!(owner_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_both_converted_operands_at_the_measured_peak() {
    // Arrange
    let value = "é\n".repeat(512);
    let baseline = controls(10 * 1024 * 1024);
    let first = OwnedOperand::convert(ScalarValue::Str(value.clone()), &baseline).expect("first");
    let second = OwnedOperand::convert(ScalarValue::Str(value.clone()), &baseline).expect("second");
    let peak = baseline.peak_query_memory_bytes();
    drop(second);
    drop(first);
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = controls(budget);
        let first =
            OwnedOperand::convert(ScalarValue::Str(value.clone()), &controls).expect("first");
        let second = OwnedOperand::convert(ScalarValue::Str(value.clone()), &controls);
        assert_eq!(second.is_ok(), succeeds);
        drop(second);
        drop(first);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
