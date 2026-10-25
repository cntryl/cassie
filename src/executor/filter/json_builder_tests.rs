use super::{evaluate, EvalContext, FunctionCall, Value};
use crate::config::CassieRuntimeLimits;
use crate::executor::filter::Expr;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::{collections::HashMap, time::Instant};

fn call(
    controls: &QueryExecutionControls,
    function: &FunctionCall,
    args: &[Value],
) -> Result<Value, crate::executor::QueryError> {
    let functions = HashMap::new();
    let row = Vec::<(String, Value)>::new();
    let context = EvalContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        local_args: None,
        session: None,
        exists: None,
        controls: Some(controls),
    };
    evaluate(function, &row, args, context).expect("selected JSON constructor")
}

#[test]
fn should_enforce_json_builder_memory_given_small_limits_when_output_is_constructed() {
    // Arrange
    let function = FunctionCall {
        name: "jsonb_build_array".into(),
        args: vec![Expr::IntegerLiteral(42), Expr::Null],
        ..FunctionCall::default()
    };
    let scarce = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 1024,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let ample = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 65536,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    // Act
    let denied = call(&scarce, &function, &[Value::Int64(42), Value::Null]);
    let accepted = call(&ample, &function, &[Value::Int64(42), Value::Null]);
    // Assert
    assert!(denied
        .expect_err("output exceeds budget")
        .to_string()
        .contains("memory budget exceeded"));
    assert_eq!(
        accepted.expect("admitted output"),
        Value::String("[42, null]".into())
    );
    assert_eq!(scarce.current_query_memory_bytes(), 0);
    assert_eq!(ample.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_json_builder_memory_given_errors_and_cancellation_when_construction_stops() {
    // Arrange
    let limits = CassieRuntimeLimits::default();
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let cancellation = QueryCancellationHandle::new();
    let cancelled =
        QueryExecutionControls::with_cancellation(&limits, Instant::now(), cancellation.clone());
    cancellation.cancel();
    let function = FunctionCall {
        name: "jsonb_build_object".into(),
        args: vec![Expr::Null, Expr::IntegerLiteral(42)],
        ..FunctionCall::default()
    };
    // Act
    let invalid = call(&controls, &function, &[Value::Null, Value::Int64(42)]);
    let interrupted = call(&cancelled, &function, &[Value::Null, Value::Int64(42)]);
    // Assert
    assert!(invalid.is_err());
    assert!(interrupted
        .expect_err("cancelled construction")
        .to_string()
        .contains("cancel"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(cancelled.current_query_memory_bytes(), 0);
}
