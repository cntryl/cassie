//! ARRAY construction moves native values and admits its structural buffers.

use super::*;
use crate::executor::batch::BatchRow;
use crate::executor::execution::graph_memory_test_observer as observer;
use crate::executor::filter::{evaluate_source_argument, EvaluationMemory};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{Expr, FunctionCall};
use crate::types::typed::TypedValue;
use crate::types::{ArrayDimension, SqlArray};
use std::collections::HashMap;
use std::time::Instant;

fn expression(arguments: Vec<Expr>) -> Expr {
    Expr::Function(Box::new(FunctionCall {
        name: crate::sql::ast::ARRAY_CONSTRUCTOR_FUNCTION.to_owned(),
        args: arguments,
        ..FunctionCall::default()
    }))
}

#[test]
fn should_construct_a_flat_numeric_array_without_reserving_numeric_formatting_workspace() {
    // Arrange
    let limits = crate::config::CassieRuntimeLimits {
        query_memory_budget_bytes: 128 * 1024,
        ..crate::config::CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let memory = EvaluationMemory::new(&controls).expect("argument owner");
    let numeric =
        crate::types::extended::numeric::parse_text(&"1".repeat(2_048)).expect("wide value");
    let params = [
        Value::Typed(TypedValue::Numeric(numeric.clone())),
        Value::Typed(TypedValue::Numeric(numeric)),
    ];
    let row = BatchRow::new(Vec::new());
    let functions = HashMap::new();

    // Act
    let value = evaluate_source_argument(
        &row,
        &expression(vec![Expr::Param(0), Expr::Param(1)]),
        &params,
        &functions,
        None,
        &controls,
        &memory,
    )
    .expect("native values need structural ARRAY workspace only");
    let retained = controls.current_query_memory_bytes();
    let Value::Array(array) = &value else {
        panic!("native array");
    };

    // Assert
    assert_eq!(array.elements(), &params);
    assert!(retained > 0);
    drop(value);
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_the_array_argument_directory_before_collecting_scalar_values() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
    );
    let memory = EvaluationMemory::new(&controls).expect("argument owner");
    let row = BatchRow::new(Vec::new());
    let functions = HashMap::new();
    let _observation = observer::start();

    // Act
    let value = evaluate_source_argument(
        &row,
        &expression(vec![Expr::IntegerLiteral(1), Expr::IntegerLiteral(2)]),
        &[],
        &functions,
        None,
        &controls,
        &memory,
    )
    .expect("two scalar values");

    // Assert
    assert!(matches!(value, Value::Array(_)));
    observer::assert_admitted(
        "before_array_arguments",
        "copy_array_arguments",
        2 * std::mem::size_of::<Value>(),
    );
}

#[test]
fn should_preserve_nested_native_array_shape() {
    // Arrange
    let child = |value| {
        Value::Array(
            SqlArray::new(
                vec![ArrayDimension {
                    length: 1,
                    lower_bound: 3,
                }],
                vec![Value::Int64(value)],
            )
            .expect("child array"),
        )
    };
    let params = [child(1), child(2)];
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
    );
    let memory = EvaluationMemory::new(&controls).expect("argument owner");
    let row = BatchRow::new(Vec::new());
    let functions = HashMap::new();

    // Act
    let value = evaluate_source_argument(
        &row,
        &expression(vec![Expr::Param(0), Expr::Param(1)]),
        &params,
        &functions,
        None,
        &controls,
        &memory,
    )
    .expect("nested arrays");

    // Assert
    let Value::Array(array) = value else {
        panic!("native array");
    };
    assert_eq!(
        array.dimensions(),
        &[
            ArrayDimension {
                length: 2,
                lower_bound: 1
            },
            ArrayDimension {
                length: 1,
                lower_bound: 3
            }
        ]
    );
    assert_eq!(array.elements(), &[Value::Int64(1), Value::Int64(2)]);
}
