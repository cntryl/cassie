use super::{
    build_array_query, evaluate, retain_builder_output, EvalContext, Expr, FunctionCall, Value,
};
use crate::config::CassieRuntimeLimits;
use crate::executor::filter::EvaluationMemory;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::types::{DataType, SqlArray};
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
fn should_retain_workspace_for_selected_json_builder_variants() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    let functions = HashMap::new();
    let row = Vec::<(String, Value)>::new();
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
    let text_array = DataType::Array(Box::new(DataType::Text));
    let array_expr = Expr::Cast {
        expr: Box::new(Expr::Column("p".into())),
        data_type: text_array.clone(),
        source_type: Some(text_array),
        identity: crate::sql::ast::CastIdentity::Scalar,
    };
    let pairs = Value::Array(
        SqlArray::from_elements(vec![Value::String("a".into()), Value::String("b".into())])
            .expect("pairs"),
    );
    let cases = [
        (
            "json_build_array",
            vec![Expr::IntegerLiteral(7)],
            vec![Value::Int64(7)],
        ),
        (
            "jsonb_build_array",
            vec![Expr::IntegerLiteral(7)],
            vec![Value::Int64(7)],
        ),
        (
            "json_build_object",
            vec![Expr::StringLiteral("a".into()), Expr::IntegerLiteral(7)],
            vec![Value::String("a".into()), Value::Int64(7)],
        ),
        (
            "jsonb_build_object",
            vec![Expr::StringLiteral("a".into()), Expr::IntegerLiteral(7)],
            vec![Value::String("a".into()), Value::Int64(7)],
        ),
        (
            "to_json",
            vec![Expr::IntegerLiteral(7)],
            vec![Value::Int64(7)],
        ),
        ("json_object", vec![array_expr.clone()], vec![pairs.clone()]),
    ];
    // Act / Assert
    for (name, args, values) in cases {
        let function = FunctionCall {
            name: name.into(),
            args,
            ..FunctionCall::default()
        };
        let output = evaluate(&function, &row, &values, context)
            .expect("selected")
            .expect("output");
        assert!(matches!(output, Value::String(_)));
        assert!(controls.current_query_memory_bytes() > 0);
    }
    let function = FunctionCall {
        name: "json_array".into(),
        args: vec![array_expr],
        ..FunctionCall::default()
    };
    let output = build_array_query(&function, &row, &[pairs], context).expect("query array");
    assert!(matches!(output, Value::String(_)));
    drop(output);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_grow_an_undersized_retained_lease_or_release_on_failure() {
    // Arrange
    for (budget, succeeds) in [(2_048, true), (1, false)] {
        let controls = controls(budget);
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
        let mut text = String::with_capacity(1_024);
        text.push('x');
        let capacity = text.capacity();
        let reservation = controls.reserve_query_memory(1).expect("reservation");
        // Act
        let output = retain_builder_output(
            Value::String(text),
            Some(reservation),
            context,
            Some(&controls),
        );
        // Assert
        assert_eq!(output.is_ok(), succeeds);
        if succeeds {
            assert!(owner.bytes() >= capacity);
        }
        drop(output);
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_completed_builder_output_after_cancellation_before_transfer() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let functions = HashMap::new();
    let row = Vec::<(String, Value)>::new();
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
    let function = FunctionCall {
        name: "json_build_array".into(),
        args: vec![Expr::IntegerLiteral(7)],
        ..FunctionCall::default()
    };
    super::cancel_after_finish_once(cancel);
    // Act
    let result = evaluate(&function, &row, &[Value::Int64(7)], context).expect("selected");
    // Assert
    assert!(result.is_err());
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_general_and_ownerless_paths_unchanged_and_reject_foreign_owner() {
    // Arrange
    let owner_controls = controls(10 * 1024 * 1024);
    let foreign = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&owner_controls).expect("owner");
    let general = EvaluationMemory::new(&owner_controls).expect("general");
    let functions = HashMap::new();
    let base = EvalContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        local_args: None,
        session: None,
        exists: None,
        controls: Some(&owner_controls),
        memory: Some(&general),
        deletion_memory: Some(&owner),
    };
    // Act / Assert
    for context in [
        base,
        EvalContext {
            memory: None,
            deletion_memory: None,
            ..base
        },
    ] {
        let reservation = owner_controls
            .reserve_query_memory(16)
            .expect("reservation");
        let result = retain_builder_output(
            Value::String("x".into()),
            Some(reservation),
            context,
            Some(&owner_controls),
        )
        .expect("fallback");
        assert!(matches!(result, Value::String(_)));
        assert_eq!(owner.bytes(), 0);
        assert_eq!(owner_controls.current_query_memory_bytes(), 0);
    }
    let reservation = foreign.reserve_query_memory(16).expect("reservation");
    let result = retain_builder_output(
        Value::String("x".into()),
        Some(reservation),
        EvalContext {
            memory: None,
            ..base
        },
        Some(&foreign),
    );
    assert!(result.is_err());
    assert_eq!(foreign.current_query_memory_bytes(), 0);
    drop(general);
    drop(owner);
    assert_eq!(owner_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_builder_null_and_error_cleanup() {
    // Arrange
    let controls = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&controls).expect("owner");
    let functions = HashMap::new();
    let row = Vec::<(String, Value)>::new();
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
    let null_function = FunctionCall {
        name: "to_json".into(),
        args: vec![Expr::Null],
        ..FunctionCall::default()
    };
    let invalid_function = FunctionCall {
        name: "jsonb_build_object".into(),
        args: vec![Expr::Null, Expr::IntegerLiteral(1)],
        ..FunctionCall::default()
    };
    // Act
    let null = evaluate(&null_function, &row, &[Value::Null], context).expect("selected");
    let invalid = evaluate(
        &invalid_function,
        &row,
        &[Value::Null, Value::Int64(1)],
        context,
    )
    .expect("selected");
    // Assert
    assert!(matches!(null, Ok(Value::Null)));
    assert!(invalid.is_err());
    assert_eq!(owner.bytes(), 0);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
