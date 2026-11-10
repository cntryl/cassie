use super::{as_json_document, into_json_document, ScalarValue};

#[test]
fn should_move_json_carrier_without_copying_its_buffer() {
    // Arrange
    let mut text = String::with_capacity(256);
    text.push_str(r#"{"a":1}"#);
    let pointer = text.as_ptr();
    let capacity = text.capacity();
    // Act
    let converted = into_json_document(ScalarValue::Json(text)).expect("JSON carrier");
    // Assert
    let ScalarValue::Json(converted) = converted else {
        panic!("JSON carrier")
    };
    assert_eq!(converted.as_ptr(), pointer);
    assert_eq!(converted.capacity(), capacity);
    assert_eq!(converted, r#"{"a":1}"#);
}

#[test]
fn should_preserve_null_scalar_conversion_and_error_results() {
    // Arrange
    let values = [
        ScalarValue::Null,
        ScalarValue::Bool(true),
        ScalarValue::Int(7),
        ScalarValue::Float(1.25),
        ScalarValue::Float(f64::NAN),
        ScalarValue::Str("é\n".into()),
        ScalarValue::Json("{}".into()),
        ScalarValue::Array(crate::types::SqlArray::from_elements(vec![]).expect("array")),
    ];
    // Act / Assert
    for value in values {
        let expected = as_json_document(&value).map_err(|error| error.to_string());
        let actual = into_json_document(value).map_err(|error| error.to_string());
        match (expected, actual) {
            (Ok(ScalarValue::Json(a)), Ok(ScalarValue::Json(b))) | (Err(a), Err(b)) => {
                assert_eq!(a, b)
            }
            (Ok(ScalarValue::Null), Ok(ScalarValue::Null)) => {}
            _ => panic!("conversion parity"),
        }
    }
}

#[test]
fn should_retain_typed_extraction_result_after_moving_source() {
    // Arrange
    use crate::config::CassieRuntimeLimits;
    use crate::executor::batch::BatchRow;
    use crate::executor::filter::{
        evaluate_expr_value_with_context, EvalContext, EvaluationMemory,
    };
    use crate::runtime::QueryExecutionControls;
    use crate::types::{DataType, Value};
    use std::{collections::HashMap, time::Instant};
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let mut row = BatchRow::new(vec![(
        "doc".into(),
        Value::Json(serde_json::json!({"a":"payload"})),
    )]);
    row.set_data_types(vec![DataType::Jsonb].into());
    let expression = crate::sql::parser::parse_expression("doc ->> 'a'").expect("expression");
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
    let result = evaluate_expr_value_with_context(&row, &expression, context).expect("extraction");
    // Assert
    assert!(matches!(&result, Value::String(text) if text == "payload"));
    assert!(controls.current_query_memory_bytes() > 0);
    drop(result);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
