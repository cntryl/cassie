use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::batch::BatchRow;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::types::Value;
use std::sync::Arc;
use std::time::Instant;

fn controls(budget: usize, cancellation: QueryCancellationHandle) -> QueryExecutionControls {
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: budget,
        query_timeout_ms: 0,
        ..CassieRuntimeLimits::default()
    };
    QueryExecutionControls::with_cancellation(&limits, Instant::now(), cancellation)
}

fn row(value: Value) -> BatchRow {
    BatchRow::from_projected_values(vec![("v".into(), value)])
}

#[test]
fn should_hold_both_source_owners_through_filter_with_lazy_consumption() {
    // Arrange
    for (query_owned, operator_owned) in [(true, false), (false, true), (true, true)] {
        let controls = controls(8192, QueryCancellationHandle::new());
        let query = Arc::new(controls.reserve_query_memory(100).expect("query"));
        let operator = Arc::new(controls.reserve_query_memory(200).expect("operator"));
        let weak_query = Arc::downgrade(&query);
        let weak_operator = Arc::downgrade(&operator);
        let mut input = row(Value::String("payload".into()));
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
        let mut source = from_select_observing(vec![input], &controls, |_| {}).expect("source");
        let rewritten = source
            .take_rows()
            .into_iter()
            .map(|row| row.into_iter().collect())
            .collect();
        source.replace_rows(rewritten);
        let mut rows = source.take_rows().into_iter();
        let first = rows.next().expect("row");
        // Assert
        assert_eq!(weak_query.upgrade().is_some(), query_owned);
        assert_eq!(weak_operator.upgrade().is_some(), operator_owned);
        drop(first);
        drop(rows);
        drop(source);
        assert!(weak_query.upgrade().is_none());
        assert!(weak_operator.upgrade().is_none());
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_source_after_partial_iterator_failure() {
    // Arrange
    let controls = controls(8192, QueryCancellationHandle::new());
    let mut weak = Vec::new();
    let rows = (0..2)
        .map(|_| {
            let owner = Arc::new(controls.reserve_query_memory(128).expect("owner"));
            weak.push(Arc::downgrade(&owner));
            row(Value::String("value".into())).with_query_memory(Some(owner))
        })
        .collect();
    let mut source = from_select_observing(rows, &controls, |_| {}).expect("source");
    // Act
    let mut iter = source.take_rows().into_iter();
    let first = iter.next().expect("first");
    assert!(weak.iter().all(|owner| owner.upgrade().is_some()));
    drop(first);
    drop(iter);
    drop(source);
    // Assert
    assert!(weak.iter().all(|owner| owner.upgrade().is_none()));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_sidecar_exactly_with_cancel_after_a_moved_row() {
    // Arrange
    let bytes = 2 * std::mem::size_of::<SourceOwners>();
    for (budget, succeeds) in [(bytes, true), (bytes - 1, false)] {
        let controls = controls(budget, QueryCancellationHandle::new());
        // Act
        let result = from_select_observing(
            vec![row(Value::Int64(1)), row(Value::Int64(2))],
            &controls,
            |_| {},
        );
        // Assert
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    let cancellation = QueryCancellationHandle::new();
    let controls = controls(8192, cancellation.clone());
    let owner = Arc::new(controls.reserve_query_memory(128).expect("source"));
    let input = row(Value::String("value".into())).with_query_memory(Some(owner));
    let mut moved = 0;
    let result = from_select_observing(vec![input], &controls, |_| {
        moved += 1;
        cancellation.cancel();
    });
    assert_eq!(moved, 1);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
