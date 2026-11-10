use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::batch::BatchRow;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::types::Value;
use std::sync::Arc;
use std::time::Instant;

fn make_controls(budget: usize, cancellation: QueryCancellationHandle) -> QueryExecutionControls {
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: budget,
        query_timeout_ms: 0,
        ..CassieRuntimeLimits::default()
    };
    QueryExecutionControls::with_cancellation(&limits, Instant::now(), cancellation)
}

fn row(values: Vec<Value>) -> BatchRow {
    BatchRow::from_projected_values(
        values
            .into_iter()
            .enumerate()
            .map(|(i, value)| (i.to_string(), value))
            .collect(),
    )
}

#[test]
fn should_keep_source_owners_until_assignment_values_are_consumed() {
    // Arrange
    for (query_owned, operator_owned) in [(true, false), (false, true), (true, true)] {
        let controls = make_controls(8192, QueryCancellationHandle::new());
        let query = Arc::new(controls.reserve_query_memory(100).expect("query"));
        let operator = Arc::new(controls.reserve_query_memory(200).expect("operator"));
        let weak_query = Arc::downgrade(&query);
        let weak_operator = Arc::downgrade(&operator);
        let mut input = row(vec![Value::String("payload".into())]);
        if query_owned {
            input = input.with_query_memory(Some(Arc::clone(&query)));
        }
        if operator_owned {
            input = input
                .retain_operator_memory(&controls, Arc::clone(&operator))
                .expect("operator");
        }
        drop(query);
        drop(operator);
        // Act
        let mut values = retain_rows_observing(vec![input], 1, &controls, |_| {}).expect("result");
        let mut iter = values.take_values().into_iter();
        let first = iter.next().expect("value");
        // Assert
        assert_eq!(first, Value::String("payload".into()));
        assert_eq!(weak_query.upgrade().is_some(), query_owned);
        assert_eq!(weak_operator.upgrade().is_some(), operator_owned);
        drop(first);
        drop(iter);
        drop(values);
        assert!(weak_query.upgrade().is_none());
        assert!(weak_operator.upgrade().is_none());
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_check_cardinality_before_budget_with_width_after_a_move() {
    // Arrange
    let controls = make_controls(0, QueryCancellationHandle::new());
    let rows = vec![row(vec![Value::Int64(1)]), row(vec![Value::Int64(2)])];
    // Act
    let result = retain_rows_observing(rows, 1, &controls, |_| {});
    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(
            crate::app::CassieError::CardinalityViolation(_)
        ))
    ));
    let controls = make_controls(8192, QueryCancellationHandle::new());
    let mut moved = 0;
    let result = retain_rows_observing(
        vec![row(vec![Value::Int64(1), Value::Int64(2)])],
        1,
        &controls,
        |_| moved += 1,
    );
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::Parse(_)))
    ));
    assert_eq!(moved, 2);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_empty_with_nonempty_directories_at_exact_budget_with_cancel_mid_move() {
    // Arrange
    let bytes = std::mem::size_of::<Value>();
    for empty in [false, true] {
        for (budget, succeeds) in [(bytes, true), (bytes - 1, false)] {
            let controls = make_controls(budget, QueryCancellationHandle::new());
            let rows = if empty {
                Vec::new()
            } else {
                vec![row(vec![Value::Int64(1)])]
            };
            // Act
            let result = retain_rows_observing(rows, 1, &controls, |_| {});
            // Assert
            assert_eq!(result.is_ok(), succeeds);
            if let Ok(mut output) = result {
                assert_eq!(
                    output.take_values(),
                    if empty {
                        vec![Value::Null]
                    } else {
                        vec![Value::Int64(1)]
                    }
                );
            }
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }
    let cancellation = QueryCancellationHandle::new();
    let controls = make_controls(8192, cancellation.clone());
    let owner = Arc::new(controls.reserve_query_memory(128).expect("source"));
    let input = row(vec![Value::String("value".into())]).with_query_memory(Some(owner));
    let mut moved = 0;
    let result = retain_rows_observing(vec![input], 1, &controls, |_| {
        moved += 1;
        cancellation.cancel();
    });
    assert_eq!(moved, 1);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
