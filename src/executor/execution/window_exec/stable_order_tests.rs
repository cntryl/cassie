//! Stable window permutation admission and cancellation witnesses.
use super::*;
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;
use std::time::Instant;

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

#[test]
fn should_preserve_input_order_for_equal_sort_keys() {
    // Arrange
    let controls = controls(1_000_000);
    let keys = [2, 1, 2, 1];

    // Act
    let order = order_by(keys.len(), &controls, |left, right| {
        Ok(keys[left].cmp(&keys[right]))
    })
    .expect("stable permutation");

    // Assert
    assert_eq!(order.values, [1, 3, 0, 2]);
    drop(order);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_both_sort_directories_before_comparison() {
    // Arrange
    let controls = controls(2 * 4 * std::mem::size_of::<usize>() - 1);
    let mut comparisons = 0;

    // Act
    let result = order_by(4, &controls, |_, _| {
        comparisons += 1;
        Ok(std::cmp::Ordering::Equal)
    });

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
    ));
    assert_eq!(comparisons, 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_stable_sort_during_a_comparison_with_release_directories() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut comparisons = 0;

    // Act
    let result = order_by(4, &controls, |_, _| {
        comparisons += 1;
        cancellation.cancel();
        Ok(std::cmp::Ordering::Equal)
    });

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::QueryCancelled))
    ));
    assert_eq!(comparisons, 1);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
