use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::batch::BatchRow;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::types::Value;
use std::time::Instant;

fn make_controls(budget: usize, cancellation: QueryCancellationHandle) -> QueryExecutionControls {
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: budget,
        query_timeout_ms: 0,
        ..CassieRuntimeLimits::default()
    };
    QueryExecutionControls::with_cancellation(&limits, Instant::now(), cancellation)
}

fn subquery(kind: SubqueryKind) -> SubqueryExpr {
    SubqueryExpr::new(
        kind,
        crate::sql::parse_statement("SELECT 1").expect("statement"),
    )
}

fn row(value: Value) -> BatchRow {
    BatchRow::from_projected_values(vec![("x".into(), value)])
}

#[test]
fn should_check_scalar_cardinality_before_destination_budget() {
    // Arrange
    let controls = make_controls(0, QueryCancellationHandle::new());
    let rows = vec![row(Value::Int64(1)), row(Value::Int64(2))];
    // Act
    let result = retain_rows_observing(rows, &subquery(SubqueryKind::Scalar), &controls, |_, _| {});
    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(
            crate::app::CassieError::CardinalityViolation(_)
        ))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_both_directories_with_release_the_result_owner() {
    // Arrange
    let controls = make_controls(4096, QueryCancellationHandle::new());
    let rows = vec![row(Value::String("hello".into())), row(Value::Int64(9))];
    // Act
    let output = retain_rows_observing(rows, &subquery(SubqueryKind::Array), &controls, |_, _| {})
        .expect("retained rows");
    // Assert
    let used = controls.current_query_memory_bytes();
    assert!(
        used >= std::mem::size_of::<SubqueryRows>()
            + 2 * std::mem::size_of::<usize>()
            + 2 * std::mem::size_of::<Vec<Value>>()
            + 2 * std::mem::size_of::<Value>()
            + 5
    );
    assert_eq!(output.rows[0][0], Value::String("hello".into()));
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);

    // The same fixture succeeds at its measured admission and fails one byte below.
    for (budget, succeeds) in [(used, true), (used - 1, false)] {
        let controls = make_controls(budget, QueryCancellationHandle::new());
        let rows = vec![row(Value::String("hello".into())), row(Value::Int64(9))];
        let result =
            retain_rows_observing(rows, &subquery(SubqueryKind::Array), &controls, |_, _| {});
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_hold_both_source_owners_through_the_actual_move() {
    // Arrange
    let controls = make_controls(8192, QueryCancellationHandle::new());
    let query = Arc::new(controls.reserve_query_memory(100).expect("query owner"));
    let operator = Arc::new(controls.reserve_query_memory(200).expect("operator owner"));
    let weak_query = Arc::downgrade(&query);
    let weak_operator = Arc::downgrade(&operator);
    let source = row(Value::String("retained".into()))
        .with_query_memory(Some(query))
        .retain_operator_memory(&controls, operator)
        .expect("attach operator");
    let mut observed = false;
    // Act
    let output = retain_rows_observing(
        vec![source],
        &subquery(SubqueryKind::Array),
        &controls,
        |_, _| {
            observed = true;
            assert!(weak_query.upgrade().is_some());
            assert!(weak_operator.upgrade().is_some());
        },
    )
    .expect("retained rows");
    // Assert
    assert!(observed);
    assert!(weak_query.upgrade().is_none());
    assert!(weak_operator.upgrade().is_none());
    assert!(controls.current_query_memory_bytes() > 0);
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_require_source_with_destination_admission_to_overlap() {
    // Arrange
    let controls = make_controls(8192, QueryCancellationHandle::new());
    let source = Arc::new(controls.reserve_query_memory(128).expect("source owner"));
    let input = row(Value::String("overlap".into())).with_query_memory(Some(source));
    // Act
    let output = retain_rows_observing(
        vec![input],
        &subquery(SubqueryKind::Array),
        &controls,
        |_, _| {},
    )
    .expect("retained rows");
    // Assert
    let output_bytes = controls.current_query_memory_bytes();
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    for (budget, succeeds) in [(128 + output_bytes, true), (127 + output_bytes, false)] {
        let controls = make_controls(budget, QueryCancellationHandle::new());
        let source = Arc::new(controls.reserve_query_memory(128).expect("source owner"));
        let input = row(Value::String("overlap".into())).with_query_memory(Some(source));
        let result = retain_rows_observing(
            vec![input],
            &subquery(SubqueryKind::Array),
            &controls,
            |_, _| {},
        );
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_destination_with_source_owners_on_move_cancellation_with_width_error() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = make_controls(8192, cancellation.clone());
    let source = Arc::new(controls.reserve_query_memory(128).expect("source owner"));
    let input = row(Value::String("payload".into())).with_query_memory(Some(source));
    let mut moves = 0;
    // Act
    let result = retain_rows_observing(
        vec![input],
        &subquery(SubqueryKind::Array),
        &controls,
        |_, _| {
            moves += 1;
            cancellation.cancel();
        },
    );
    // Assert
    assert_eq!(moves, 1);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);

    let controls = make_controls(8192, QueryCancellationHandle::new());
    let rows = vec![
        row(Value::Int64(1)),
        BatchRow::from_projected_values(vec![
            ("x".into(), Value::Int64(2)),
            ("y".into(), Value::Int64(3)),
        ]),
    ];
    let result = retain_rows_observing(rows, &subquery(SubqueryKind::Array), &controls, |_, _| {});
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::Diagnostic(_)))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_treat_float_power_range_failures_as_non_interruptions() {
    // Arrange
    let errors = [
        QueryError::FloatPowerOverflow,
        QueryError::FloatPowerUnderflow,
    ];

    // Act / Assert
    for error in &errors {
        // Assert
        assert!(!is_interruption(error));
    }
    assert!(is_interruption(&QueryError::Cassie(
        crate::app::CassieError::QueryCancelled
    )));
    assert!(is_interruption(&QueryError::Cassie(
        crate::app::CassieError::DeadlineExceeded
    )));
}
