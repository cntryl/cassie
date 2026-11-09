use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn typed_json_row(value: serde_json::Value) -> BatchRow {
    let mut row = BatchRow::new(vec![("doc".into(), Value::Json(value))]);
    row.set_data_types(vec![crate::types::DataType::Jsonb].into());
    row
}

#[test]
fn should_retain_controlled_deletion_output_through_value_handoff() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let owner = EvaluationMemory::new(&controls).expect("deletion owner");
    let row = typed_json_row(serde_json::json!({"drop": 1, "keep": "payload"}));
    let expression = crate::sql::parser::parse_expression("doc - 'drop'").expect("expression");
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
    let result = evaluate_expr_value_with_context(&row, &expression, context).expect("deletion");
    // Assert
    let Value::String(text) = &result else {
        panic!("JSON carrier")
    };
    assert_eq!(text, r#"{"keep": "payload"}"#);
    assert!(controls.current_query_memory_bytes() >= text.capacity());
    drop(result);
    assert!(controls.current_query_memory_bytes() > 0);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_controlled_deletion_owner_after_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let owner = EvaluationMemory::new(&controls).expect("deletion owner");
    let row = typed_json_row(serde_json::json!({"drop": 1}));
    let expression = crate::sql::parser::parse_expression("doc - 'drop'").expect("expression");
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
    cancel.cancel();
    // Act
    let result = evaluate_expr_value_with_context(&row, &expression, context);
    // Assert
    assert!(result.is_err());
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_the_per_row_deletion_owner_after_filtering() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let row = typed_json_row(serde_json::json!({"drop":1,"keep":2}));
    let expression =
        crate::sql::parser::parse_expression("(doc - 'drop') ? 'keep'").expect("predicate");
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
    let rows =
        super::filtering::filter_rows_in_context(vec![row], &expression, context).expect("filter");
    // Assert
    assert_eq!(rows.len(), 1);
    assert!(controls.peak_query_memory_bytes() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_a_foreign_owner_and_release_the_new_output() {
    // Arrange
    let first =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let second =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let owner = EvaluationMemory::new(&first).expect("owner");
    let left = ScalarValue::Json(r#"{"drop":1,"keep":2}"#.into());
    let right = ScalarValue::Str("drop".into());
    // Act
    let result = super::json_delete::controlled_operator(&left, &right, &second, &owner);
    // Assert
    assert!(result.is_err());
    assert_eq!(second.current_query_memory_bytes(), 0);
    drop(owner);
    assert_eq!(first.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_nested_deletion_outputs_owned_until_the_expression_returns() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let row = typed_json_row(serde_json::json!({"drop": 1, "keep": 2, "last": 3}));
    let expression =
        crate::sql::parser::parse_expression("(doc - 'drop') - 'keep'").expect("expression");
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
    let result =
        evaluate_expr_value_with_context(&row, &expression, context).expect("nested deletion");
    // Assert
    assert!(matches!(&result, Value::String(text) if text == r#"{"last": 3}"#));
    assert!(controls.current_query_memory_bytes() > 0);
    drop(result);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_transfer_native_path_deletion_output_to_the_expression_owner() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let left = ScalarValue::Json(r#"{"a":{"b":1,"c":2}}"#.into());
    let right = ScalarValue::Array(
        crate::types::SqlArray::from_elements(vec![
            Value::String("a".into()),
            Value::String("b".into()),
        ])
        .expect("path"),
    );
    // Act
    let result = super::json_delete::controlled_path(&left, &right, &controls, &owner)
        .expect("controlled path")
        .expect("native path");
    // Assert
    assert!(matches!(&result, ScalarValue::Json(text) if text == r#"{"a": {"c": 2}}"#));
    assert!(controls.current_query_memory_bytes() > 0);
    drop(result);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_use_session_controls_for_the_regular_filter_entrypoint() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let session = CassieSession::new("root".into(), None)
        .with_statement_text("SELECT 1", &controls)
        .expect("session");
    let baseline = controls.current_query_memory_bytes();
    let row = typed_json_row(serde_json::json!({"drop":1,"keep":2}));
    let expression =
        crate::sql::parser::parse_expression("(doc - 'drop') ? 'keep'").expect("predicate");
    let functions = HashMap::new();
    // Act
    let rows = super::filtering::filter_rows(
        vec![row],
        &expression,
        &[],
        None,
        &functions,
        Some(&session),
    )
    .expect("filter");
    // Assert
    assert_eq!(rows.len(), 1);
    assert!(controls.peak_query_memory_bytes() > baseline);
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(session);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_dynamic_json_subtraction_owned_with_or_without_a_json_column_type() {
    // Arrange
    let expression = crate::sql::parser::parse_expression("doc - 'drop'").expect("expression");
    let functions = HashMap::new();
    for data_type in [
        None,
        Some(crate::types::DataType::Text),
        Some(crate::types::DataType::Jsonb),
    ] {
        let controls =
            QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
        let owner = EvaluationMemory::new(&controls).expect("owner");
        let mut row = BatchRow::new(vec![(
            "doc".into(),
            Value::Json(serde_json::json!({"drop":1,"keep":2})),
        )]);
        if let Some(data_type) = data_type {
            row.set_data_types(vec![data_type].into());
        }
        let legacy = EvalContext {
            params: &[],
            search_context: None,
            user_functions: &functions,
            local_args: None,
            session: None,
            exists: None,
            controls: None,
            memory: None,
            deletion_memory: None,
        };
        let controlled = EvalContext {
            controls: Some(&controls),
            deletion_memory: Some(&owner),
            ..legacy
        };
        // Act
        let expected = evaluate_expr_value_with_context(&row, &expression, legacy).expect("legacy");
        let actual =
            evaluate_expr_value_with_context(&row, &expression, controlled).expect("controlled");
        // Assert
        assert_eq!(actual, expected);
        assert!(controls.current_query_memory_bytes() > 0);
        drop(actual);
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_route_a_typed_json_column_and_native_path_through_the_owner() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let row = typed_json_row(serde_json::json!({"a":{"b":1,"c":2}}));
    let path = Value::Array(
        crate::types::SqlArray::from_elements(vec![
            Value::String("a".into()),
            Value::String("b".into()),
        ])
        .expect("path"),
    );
    let params = [path];
    let functions = HashMap::new();
    let expression = crate::sql::parser::parse_expression("doc #- $1").expect("path expression");
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
    // Act
    let value = evaluate_expr_value_with_context(&row, &expression, context).expect("deletion");
    // Assert
    assert!(matches!(&value, Value::String(text) if text == r#"{"a": {"c": 2}}"#));
    assert!(controls.current_query_memory_bytes() > 0);
    drop(value);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_route_a_typed_json_column_and_flat_text_path_through_the_owner() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let row = typed_json_row(serde_json::json!({"a":{"b":1,"c":2}}));
    let params = [Value::String("{a,b}".into())];
    let functions = HashMap::new();
    let expression = crate::sql::parser::parse_expression("doc #- $1").expect("path expression");
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
    // Act
    let value = evaluate_expr_value_with_context(&row, &expression, context).expect("deletion");
    // Assert
    assert!(matches!(&value, Value::String(text) if text == r#"{"a": {"c": 2}}"#));
    assert!(controls.current_query_memory_bytes() > 0);
    drop(value);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_route_a_typed_json_column_and_json_array_path_through_the_owner() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let row = typed_json_row(serde_json::json!({"a":{"b":1,"c":2}}));
    let params = [Value::String(r#"["a","b"]"#.into())];
    let functions = HashMap::new();
    let expression = crate::sql::parser::parse_expression("doc #- $1").expect("path expression");
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
    // Act
    let value = evaluate_expr_value_with_context(&row, &expression, context).expect("deletion");
    // Assert
    assert!(matches!(&value, Value::String(text) if text == r#"{"a": {"c": 2}}"#));
    assert!(controls.current_query_memory_bytes() > 0);
    drop(value);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_a_source_argument_deletion_in_its_existing_expression_owner() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let row = typed_json_row(serde_json::json!({"drop":1,"keep":"payload"}));
    let expression = crate::sql::parser::parse_expression("doc - 'drop'").expect("expression");
    let functions = HashMap::new();
    // Act
    let result =
        evaluate_source_argument(&row, &expression, &[], &functions, None, &controls, &owner)
            .expect("source expression");
    // Assert
    assert!(matches!(&result, Value::String(text) if text == r#"{"keep": "payload"}"#));
    assert!(controls.current_query_memory_bytes() > 0);
    drop(result);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_a_resolved_source_argument_deletion_in_its_existing_owner() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let row = typed_json_row(serde_json::json!({"drop":1,"keep":2}));
    let expression = crate::sql::parser::parse_expression("doc - 'drop'").expect("expression");
    let functions = HashMap::new();
    let resolver = |expr: &Expr| Ok(expr.clone());
    let context = ExistsValueContext {
        params: &[],
        search: None,
        functions: &functions,
        session: None,
        resolver: &resolver,
    };
    // Act
    let result = evaluate_source_argument_resolving(&row, &expression, context, &controls, &owner)
        .expect("source expression");
    // Assert
    assert!(matches!(&result, Value::String(text) if text == r#"{"keep": 2}"#));
    assert!(controls.current_query_memory_bytes() > 0);
    drop(result);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_dynamic_array_subtraction_error_but_allow_typed_json_deletion() {
    // Arrange
    let expression = crate::sql::parser::parse_expression("doc - $1").expect("expression");
    let keys = Value::Array(
        crate::types::SqlArray::from_elements(vec![Value::String("drop".into())]).expect("keys"),
    );
    let params = [keys];
    let functions = HashMap::new();
    for data_type in [
        None,
        Some(crate::types::DataType::Text),
        Some(crate::types::DataType::Jsonb),
    ] {
        let controls =
            QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
        let owner = EvaluationMemory::new(&controls).expect("owner");
        let mut row = BatchRow::new(vec![(
            "doc".into(),
            Value::Json(serde_json::json!({"drop":1,"keep":2})),
        )]);
        if let Some(data_type) = &data_type {
            row.set_data_types(vec![data_type.clone()].into());
        }
        let legacy = EvalContext {
            params: &params,
            search_context: None,
            user_functions: &functions,
            local_args: None,
            session: None,
            exists: None,
            controls: None,
            memory: None,
            deletion_memory: None,
        };
        let controlled = EvalContext {
            controls: Some(&controls),
            deletion_memory: Some(&owner),
            ..legacy
        };
        // Act
        let expected = evaluate_expr_value_with_context(&row, &expression, legacy)
            .map_err(|error| error.to_string());
        let actual = evaluate_expr_value_with_context(&row, &expression, controlled)
            .map_err(|error| error.to_string());
        // Assert
        assert_eq!(actual, expected);
        if matches!(data_type, Some(crate::types::DataType::Jsonb)) {
            assert!(controls.current_query_memory_bytes() > 0);
        } else {
            assert!(actual.is_err());
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
        drop(actual);
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_retain_controlled_json_extraction_output_through_value_handoff() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let owner = EvaluationMemory::new(&controls).expect("result owner");
    let row = typed_json_row(serde_json::json!({"a":{"b":"payload"}}));
    let expression = crate::sql::parser::parse_expression("doc #>> $1").expect("expression");
    let params = [Value::Array(
        crate::types::SqlArray::from_elements(vec![
            Value::String("a".into()),
            Value::String("b".into()),
        ])
        .expect("path"),
    )];
    let functions = HashMap::new();
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
    // Act
    let result = evaluate_expr_value_with_context(&row, &expression, context).expect("extraction");
    // Assert
    assert!(matches!(&result, Value::String(text) if text == "payload"));
    assert!(controls.current_query_memory_bytes() > 0);
    drop(result);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_legacy_controlled_extraction_results_and_owner_release() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let cases = [
        (
            r#"{"a":1,"\u0061":2}"#,
            ScalarValue::Str("a".into()),
            false,
            false,
        ),
        (r#"[1,{"n":1e+3}]"#, ScalarValue::Int(-1), false, true),
        (r#"["a\nb"]"#, ScalarValue::Int(0), true, false),
        ("null", ScalarValue::Int(0), true, true),
        ("9", ScalarValue::Int(-1), false, true),
        ("9", ScalarValue::Int(-1), false, false),
    ];
    // Act / Assert
    for (source, selector, as_text, jsonb) in cases {
        let document = ScalarValue::Json(source.into());
        let expected = super::json_extract::evaluate(&document, &selector, false, as_text, jsonb)
            .expect("legacy extraction");
        let owner = EvaluationMemory::new(&controls).expect("owner");
        let actual = super::json_delete::controlled_extraction(
            &document, &selector, false, as_text, jsonb, &controls, &owner,
        )
        .expect("controlled extraction")
        .expect("supported selector");
        match (&actual, &expected) {
            (ScalarValue::Null, ScalarValue::Null) => {}
            (ScalarValue::Str(actual), ScalarValue::Str(expected))
            | (ScalarValue::Json(actual), ScalarValue::Json(expected)) => {
                assert_eq!(actual, expected)
            }
            _ => panic!("extraction mismatch for {source}"),
        }
        drop(actual);
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_reject_foreign_extraction_owner_without_leaking_output() {
    // Arrange
    let first =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let second =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let owner = EvaluationMemory::new(&first).expect("owner");
    let document = ScalarValue::Json(r#"{"a":"payload"}"#.into());
    let selector = ScalarValue::Str("a".into());
    // Act
    let result = super::json_delete::controlled_extraction(
        &document, &selector, false, true, false, &second, &owner,
    );
    // Assert
    assert!(result.is_err());
    assert_eq!(second.current_query_memory_bytes(), 0);
    drop(owner);
    assert_eq!(first.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_extraction_null_fallback_and_native_path_error_order() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let malformed = ScalarValue::Json("[}".into());
    let unsupported = ScalarValue::Bool(true);
    let invalid_path = ScalarValue::Array(
        crate::types::SqlArray::from_elements(vec![Value::Int64(1)]).expect("path"),
    );
    // Act / Assert
    for (document, selector, path) in [
        (&ScalarValue::Null, &unsupported, false),
        (&malformed, &ScalarValue::Null, false),
    ] {
        let expected =
            super::json_extract::evaluate(document, selector, path, false, false).expect("legacy");
        let actual = super::json_delete::controlled_extraction(
            document, selector, path, false, false, &controls, &owner,
        )
        .expect("controlled")
        .expect("supported null");
        assert!(matches!(
            (actual, expected),
            (ScalarValue::Null, ScalarValue::Null)
        ));
    }
    for (selector, path) in [(&unsupported, false), (&invalid_path, true)] {
        assert!(super::json_delete::controlled_extraction(
            &malformed, selector, path, false, false, &controls, &owner
        )
        .expect("fallback")
        .is_none());
        let error = super::json_extract::evaluate(&malformed, selector, path, false, false)
            .expect_err("legacy error")
            .to_string();
        if path {
            assert!(error.contains("incompatible operands"));
        } else {
            assert!(error.contains("invalid JSON document"));
        }
    }
    let empty_path =
        ScalarValue::Array(crate::types::SqlArray::from_elements(vec![]).expect("path"));
    let document = ScalarValue::Json(" {\"a\":1} ".into());
    let expected =
        super::json_extract::evaluate(&document, &empty_path, true, false, false).expect("legacy");
    let actual = super::json_delete::controlled_extraction(
        &document,
        &empty_path,
        true,
        false,
        false,
        &controls,
        &owner,
    )
    .expect("controlled")
    .expect("native path");
    assert!(matches!((actual, expected), (ScalarValue::Json(a), ScalarValue::Json(b)) if a == b));
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_extraction_owner_live_in_controlled_where() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let row = typed_json_row(serde_json::json!({"a":"payload"}));
    let expression =
        crate::sql::parser::parse_expression("doc ->> 'a' = 'payload'").expect("predicate");
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
    let rows =
        super::filtering::filter_rows_in_context(vec![row], &expression, context).expect("filter");
    // Assert
    assert_eq!(rows.len(), 1);
    assert!(controls.peak_query_memory_bytes() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
