use super::{eval_cast_expr, last_json_input_pointer};
use crate::config::CassieRuntimeLimits;
use crate::executor::batch::BatchRow;
use crate::executor::filter::{EvalContext, EvaluationMemory, QueryError, ScalarValue};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{CastIdentity, Expr};
use crate::types::{DataType, Value};
use std::{collections::HashMap, time::Instant};

fn run(
    row: &BatchRow,
    expression: &Expr,
    target: &DataType,
    source: &DataType,
    controls: &QueryExecutionControls,
    owner: &EvaluationMemory,
) -> Result<ScalarValue, QueryError> {
    let functions = HashMap::new();
    let context = EvalContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        local_args: None,
        session: None,
        exists: None,
        controls: Some(controls),
        memory: None,
        deletion_memory: Some(owner),
    };
    eval_cast_expr(
        row,
        expression,
        target,
        Some(source),
        CastIdentity::Scalar,
        context,
    )
}

#[test]
fn should_move_json_cast_input_with_retain_the_deletion_owner() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let mut row = BatchRow::new(vec![(
        "doc".into(),
        Value::Json(serde_json::json!({"drop":1,"keep":"payload"})),
    )]);
    row.set_data_types(vec![DataType::Json].into());
    let expression = crate::sql::parser::parse_expression("doc - 'drop'").expect("expression");
    // Act
    let result = run(
        &row,
        &expression,
        &DataType::Json,
        &DataType::Json,
        &controls,
        &owner,
    )
    .expect("cast");
    let pointer = last_json_input_pointer();
    // Assert
    let ScalarValue::Json(text) = &result else {
        panic!("JSON result")
    };
    assert_eq!(text.as_ptr(), pointer);
    assert_eq!(text, r#"{"keep": "payload"}"#);
    assert!(controls.current_query_memory_bytes() >= text.capacity());
    drop(result);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_json_cast_null_scalar_with_jsonb_canonicalization() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let expression = Expr::Column("doc".into());
    // Act / Assert
    let null = BatchRow::new(vec![("doc".into(), Value::Null)]);
    // Assert
    assert!(matches!(
        run(
            &null,
            &expression,
            &DataType::Json,
            &DataType::Json,
            &controls,
            &owner
        ),
        Ok(ScalarValue::Null)
    ));
    let scalar = BatchRow::new(vec![("doc".into(), Value::Int64(7))]);
    assert!(
        matches!(run(&scalar, &expression, &DataType::Json, &DataType::Json, &controls, &owner), Ok(ScalarValue::Json(text)) if text == "7")
    );
    let object = BatchRow::new(vec![(
        "doc".into(),
        Value::Json(serde_json::json!({"b":1,"a":2})),
    )]);
    assert!(
        matches!(run(&object, &expression, &DataType::Jsonb, &DataType::Json, &controls, &owner), Ok(ScalarValue::Json(text)) if text == r#"{"a": 2, "b": 1}"#)
    );
    assert!(
        matches!(run(&object, &expression, &DataType::Json, &DataType::Jsonb, &controls, &owner), Ok(ScalarValue::Json(text)) if text == r#"{"a": 2, "b": 1}"#)
    );
    let nul = BatchRow::new(vec![("doc".into(), Value::Json(serde_json::json!("\0")))]);
    let error = run(
        &nul,
        &expression,
        &DataType::Jsonb,
        &DataType::Json,
        &controls,
        &owner,
    )
    .expect_err("NUL error");
    assert!(
        matches!(error, QueryError::Cassie(crate::app::CassieError::TypeInput(error)) if error.sqlstate == "22P05")
    );
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
