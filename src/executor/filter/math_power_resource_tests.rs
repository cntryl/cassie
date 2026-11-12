use super::*;
use crate::executor::filter::EvaluationMemory;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::types::extended::numeric::Numeric;
use std::time::Instant;

fn controls_with_budget(bytes: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: bytes,
            ..crate::config::CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

#[test]
fn should_reject_numeric_power_before_allocating_an_unadmitted_result() {
    // Arrange
    let controls = controls_with_budget(8 * 1024);
    let memory = EvaluationMemory::new(&controls).expect("expression owner");
    let row = crate::executor::batch::BatchRow::from_projected_values(Vec::new());
    let expression = crate::sql::parser::parse_expression("power(2::numeric, 100000::numeric)")
        .expect("numeric power expression");

    // Act
    let result = crate::executor::filter::evaluation::evaluate_source_argument(
        &row,
        &expression,
        &[],
        &std::collections::HashMap::new(),
        None,
        &controls,
        &memory,
    );

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
            _
        )))
    ));
    drop(result);
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_filter_row_when_numeric_power_workspace_exceeds_the_query_budget() {
    // Arrange
    let controls = controls_with_budget(16 * 1024);
    let row = crate::executor::batch::BatchRow::from_projected_values(Vec::new());
    let expression =
        crate::sql::parser::parse_expression("power(2::numeric, 100000::numeric) > 0::numeric")
            .expect("numeric power predicate");

    // Act
    let result = crate::executor::filter::filter_batches_with_controls(
        vec![vec![row]],
        &expression,
        &[],
        None,
        &std::collections::HashMap::new(),
        None,
        &controls,
    );

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
            _
        )))
    ));
    drop(result);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_numeric_power_memory_owned_until_the_expression_owner_drops() {
    // Arrange
    let controls = controls_with_budget(1024 * 1024);
    let memory = EvaluationMemory::new(&controls).expect("expression owner");
    let row = crate::executor::batch::BatchRow::from_projected_values(Vec::new());
    let expression = crate::sql::parser::parse_expression("power(2::numeric, 100000::numeric)")
        .expect("numeric power expression");

    // Act
    let result = crate::executor::filter::evaluation::evaluate_source_argument(
        &row,
        &expression,
        &[],
        &std::collections::HashMap::new(),
        None,
        &controls,
        &memory,
    )
    .expect("admitted numeric power");
    let result_bytes = match &result {
        Value::Typed(value @ TypedValue::Numeric(Numeric::Finite { .. })) => value.heap_bytes(),
        other => panic!("expected an exact numeric result, got {other:?}"),
    };
    let retained = controls.current_query_memory_bytes();

    // Assert
    assert_eq!(retained, result_bytes);
    drop(result);
    assert_eq!(controls.current_query_memory_bytes(), retained);
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_numeric_power_operator_output_without_retaining_its_operands() {
    // Arrange
    let controls = controls_with_budget(1024 * 1024);
    let memory = EvaluationMemory::new(&controls).expect("expression owner");
    let row = crate::executor::batch::BatchRow::from_projected_values(Vec::new());
    let expression = crate::sql::parser::parse_expression("2::numeric ^ 8::numeric")
        .expect("numeric power operator");

    // Act
    let result = crate::executor::filter::evaluation::evaluate_source_argument(
        &row,
        &expression,
        &[],
        &std::collections::HashMap::new(),
        None,
        &controls,
        &memory,
    )
    .expect("admitted numeric power operator");
    let result_bytes = match &result {
        Value::Typed(value @ TypedValue::Numeric(Numeric::Finite { .. })) => value.heap_bytes(),
        other => panic!("expected an exact numeric result, got {other:?}"),
    };

    // Assert
    assert_eq!(memory.bytes(), result_bytes);
    drop(result);
    assert_eq!(memory.bytes(), result_bytes);
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_stop_numeric_power_when_its_request_is_canceled() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let memory = EvaluationMemory::new(&controls).expect("expression owner");
    let args = [
        Value::Typed(TypedValue::Numeric(Numeric::from_i64(2))),
        Value::Typed(TypedValue::Numeric(Numeric::from_i64(100_000))),
    ];
    cancellation.cancel();

    // Act
    let result = evaluate("power", &args, Some(&controls), Some(&memory));

    // Assert
    assert!(matches!(
        result,
        Some(Err(QueryError::Cassie(
            crate::app::CassieError::QueryCanceled
        )))
    ));
    drop(result);
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_check_for_cancellation_between_numeric_power_kernel_steps() {
    // Arrange
    let base = Numeric::from_i64(2);
    let exponent = Numeric::from_i64(100_000);
    let mut checkpoints = 0;

    // Act
    let result =
        crate::types::extended::numeric::math::power_with_check(&base, &exponent, &mut || {
            checkpoints += 1;
            if checkpoints == 4 {
                Err("canceled")
            } else {
                Ok(())
            }
        });

    // Assert
    assert!(matches!(
        result,
        Err(crate::types::extended::numeric::math::PowerError::Interruption("canceled"))
    ));
    assert_eq!(checkpoints, 4);
}
