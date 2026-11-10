//! Deferred direct object-key argument ownership probes.
use crate::config::CassieRuntimeLimits;
use crate::executor::batch::BatchRow;
use crate::executor::filter::{evaluate_expr_value_with_context, EvalContext, EvaluationMemory};
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::types::Value;
use std::{collections::HashMap, time::Instant};

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}
fn context<'a>(
    controls: &'a QueryExecutionControls,
    owner: Option<&'a EvaluationMemory>,
    functions: &'a HashMap<String, crate::catalog::FunctionMeta>,
    params: &'a [Value],
) -> EvalContext<'a> {
    EvalContext {
        params,
        search_context: None,
        user_functions: functions,
        local_args: None,
        session: None,
        exists: None,
        controls: Some(controls),
        memory: None,
        deletion_memory: owner,
    }
}

#[test]
fn should_avoid_direct_column_with_parameter_value_clones_only_on_selected_route() {
    // Arrange
    let controls = controls();
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let row = BatchRow::new(vec![(
        "doc".into(),
        Value::Json(serde_json::json!({"a":1})),
    )]);
    let params = [Value::Json(serde_json::json!({"b":2}))];
    let functions = HashMap::new();
    for (sql, expected) in [
        ("json_object_keys(doc)", "a"),
        ("jsonb_object_keys($1)", "b"),
    ] {
        let expression = crate::sql::parser::parse_expression(sql).expect("expression");
        super::take_input_value_clone_count();
        // Act
        let value = evaluate_expr_value_with_context(
            &row,
            &expression,
            context(&controls, Some(&owner), &functions, &params),
        )
        .expect("result");
        // Assert
        assert_eq!(value, Value::Json(serde_json::json!([expected])));
        assert_eq!(super::take_input_value_clone_count(), 0, "{sql}");
        drop(value);
        let fallback = evaluate_expr_value_with_context(
            &row,
            &expression,
            context(&controls, None, &functions, &params),
        )
        .expect("fallback");
        assert_eq!(fallback, Value::Json(serde_json::json!([expected])));
        assert_eq!(super::take_input_value_clone_count(), 1, "{sql}");
        let without_controls = EvalContext {
            controls: None,
            ..context(&controls, Some(&owner), &functions, &params)
        };
        let legacy = evaluate_expr_value_with_context(&row, &expression, without_controls)
            .expect("no controls");
        assert_eq!(legacy, Value::Json(serde_json::json!([expected])));
        assert_eq!(super::take_input_value_clone_count(), 1, "{sql}");
    }
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_local_argument_precedence_missing_null_with_builtin_dispatch() {
    // Arrange
    let controls = controls();
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let row = BatchRow::new(vec![(
        "doc".into(),
        Value::Json(serde_json::json!({"row":1})),
    )]);
    let local = HashMap::from([("doc".into(), Value::Json(serde_json::json!({"local":1})))]);
    let functions = HashMap::from([(
        "json_object_keys".into(),
        crate::catalog::FunctionMeta {
            name: "json_object_keys".into(),
            args: vec![],
            return_type: crate::types::DataType::Text,
            volatility: crate::catalog::Volatility::Immutable,
            body: "'override'".into(),
        },
    )]);
    let params = [];
    let mut context = context(&controls, Some(&owner), &functions, &params);
    context.local_args = Some(&local);
    let expression =
        crate::sql::parser::parse_expression("json_object_keys(doc)").expect("expression");
    let missing =
        crate::sql::parser::parse_expression("json_object_keys(missing)").expect("expression");
    // Act
    let value = evaluate_expr_value_with_context(&row, &expression, context).expect("result");
    let null = evaluate_expr_value_with_context(&row, &missing, context).expect("null");
    // Assert
    assert_eq!(value, Value::Json(serde_json::json!(["local"])));
    assert_eq!(null, Value::Null);
    drop(value);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_borrowed_source_after_actual_key_copy_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let row = BatchRow::new(vec![(
        "doc".into(),
        Value::Json(serde_json::json!({"long-key":1})),
    )]);
    let functions = HashMap::new();
    let expression =
        crate::sql::parser::parse_expression("json_object_keys(doc)").expect("expression");
    crate::runtime::accounted::json_arena::cancel_object_keys_copy_once(cancel.clone());
    // Act
    let result = evaluate_expr_value_with_context(
        &row,
        &expression,
        context(&controls, Some(&owner), &functions, &[]),
    );
    // Assert
    assert!(cancel.is_cancelled());
    assert!(result.is_err());
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_session_scratch_with_source_lease_through_borrowed_result() {
    // Arrange
    let controls = controls();
    let session = crate::app::CassieSession::new("root".into(), None)
        .with_statement_text("SELECT json_object_keys(doc)", &controls)
        .expect("session");
    let session_bytes = controls.current_query_memory_bytes();
    let source_memory = controls.reserve_query_memory(128).expect("source");
    let row = BatchRow::new(vec![(
        "doc".into(),
        Value::Json(serde_json::json!({"a":1})),
    )])
    .with_query_memory(Some(std::sync::Arc::new(source_memory)));
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let functions = HashMap::new();
    let expression =
        crate::sql::parser::parse_expression("json_object_keys(doc)").expect("expression");
    let mut context = context(&controls, Some(&owner), &functions, &[]);
    context.session = Some(&session);
    super::take_input_value_clone_count();
    // Act
    let value = evaluate_expr_value_with_context(&row, &expression, context).expect("result");
    // Assert
    assert_eq!(value, Value::Json(serde_json::json!(["a"])));
    assert_eq!(super::take_input_value_clone_count(), 0);
    assert_eq!(
        controls.current_query_memory_bytes(),
        session_bytes + 128 + owner.bytes()
    );
    drop(row);
    assert_eq!(
        controls.current_query_memory_bytes(),
        session_bytes + owner.bytes()
    );
    drop(value);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), session_bytes);
    drop(session);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_general_owner_fallback_with_foreign_tracker_failure() {
    // Arrange
    let controls = controls();
    let foreign = self::controls();
    let general = EvaluationMemory::new(&controls).expect("general");
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let wrong_owner = EvaluationMemory::new(&foreign).expect("foreign owner");
    let row = BatchRow::new(vec![(
        "doc".into(),
        Value::Json(serde_json::json!({"a":1})),
    )]);
    let functions = HashMap::new();
    let expression =
        crate::sql::parser::parse_expression("json_object_keys(doc)").expect("expression");
    let mut general_context = context(&controls, Some(&owner), &functions, &[]);
    general_context.memory = Some(&general);
    super::take_input_value_clone_count();
    // Act
    let legacy =
        evaluate_expr_value_with_context(&row, &expression, general_context).expect("general");
    let clones = super::take_input_value_clone_count();
    let error = evaluate_expr_value_with_context(
        &row,
        &expression,
        context(&controls, Some(&wrong_owner), &functions, &[]),
    );
    // Assert
    assert_eq!(legacy, Value::Json(serde_json::json!(["a"])));
    assert_eq!(clones, 1);
    assert!(error.is_err());
    assert_eq!(wrong_owner.bytes(), 0);
    drop(legacy);
    drop(general);
    drop(owner);
    drop(wrong_owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(foreign.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_qualified_complex_with_invalid_arity_on_owned_argument_route() {
    // Arrange
    let controls = controls();
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let row = BatchRow::new(vec![(
        "doc".into(),
        Value::Json(serde_json::json!({"a":1})),
    )]);
    let functions = HashMap::new();
    for (sql, succeeds) in [
        ("pg_catalog.json_object_keys(doc)", true),
        ("json_object_keys(coalesce(doc,doc))", true),
        ("json_object_keys(doc,doc)", false),
    ] {
        let expression = crate::sql::parser::parse_expression(sql).expect("expression");
        super::take_input_value_clone_count();
        // Act
        let result = evaluate_expr_value_with_context(
            &row,
            &expression,
            context(&controls, Some(&owner), &functions, &[]),
        );
        // Assert
        assert_eq!(result.is_ok(), succeeds, "{sql}");
        assert!(super::take_input_value_clone_count() > 0, "{sql}");
        drop(result);
    }
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
