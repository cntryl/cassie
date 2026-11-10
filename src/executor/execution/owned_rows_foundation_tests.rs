//! Test-only proof probes; no production executor path uses this foundation.
use super::owned_rows as owned;

use super::{BatchRow, QueryExecutionControls, Value};
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;
use std::time::Instant;

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
fn rows(controls: &QueryExecutionControls, count: usize, capacity: usize) -> owned::OwnedRows {
    let base = controls.reserve_query_memory(0).expect("tracker");
    let (mut rows, memory) =
        crate::runtime::owned_memory::try_reserve_vec(capacity, &base).expect("directory");
    for index in 0..count {
        rows.push(BatchRow::new(vec![(
            "n".into(),
            Value::Int64(i64::try_from(index).expect("fixture index")),
        )]));
    }
    owned::OwnedRows::new(rows, memory, controls).expect("owner")
}

#[test]
fn should_retain_empty_allocated_directory_until_owner_drops() {
    // Arrange
    let controls = controls(1024 * 1024);
    let owned = rows(&controls, 0, 4);
    let bytes = controls.current_query_memory_bytes();
    // Act
    let iterator = owned.into_iter();
    // Assert
    assert!(bytes > 0);
    assert_eq!(controls.current_query_memory_bytes(), bytes);
    drop(iterator);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_foreign_tracker_with_undercovered_directory() {
    // Arrange
    let source = controls(1024 * 1024);
    let other = controls(1024 * 1024);
    let base = source.reserve_query_memory(0).expect("tracker");
    let (directory, memory) =
        crate::runtime::owned_memory::try_reserve_vec::<BatchRow>(4, &base).expect("directory");
    // Act
    let foreign = owned::OwnedRows::new(directory, memory, &other);
    let directory = Vec::<BatchRow>::with_capacity(4);
    let insufficient = source.reserve_query_memory(0).expect("lease");
    let undercovered = owned::OwnedRows::new(directory, insufficient, &source);
    // Assert
    assert!(foreign.is_err());
    assert!(undercovered.is_err());
    assert_eq!(source.current_query_memory_bytes(), 0);
    assert_eq!(other.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_source_iterator_on_partial_move_with_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let mut source = rows(&controls, 2, 4).into_iter();
    let bytes = controls.current_query_memory_bytes();
    // Act
    let result = source.next_observing(&controls, || cancel.cancel());
    // Assert
    assert!(result.is_err());
    assert!(cancel.is_cancelled());
    assert_eq!(controls.current_query_memory_bytes(), bytes);
    drop(source);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_iterator_owner_on_unwind_after_partial_consumption() {
    // Arrange
    let controls = controls(1024 * 1024);
    let owned = rows(&controls, 2, 4);
    // Act
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut iterator = owned.into_iter();
        drop(iterator.next_observing(&controls, || {}).expect("row"));
        panic!("injected error");
    }));
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_overlap_independently_admitted_destination_until_input_iterator_drops() {
    // Arrange
    let generous = controls(1024 * 1024);
    let mut iterator = rows(&generous, 1, 4).into_iter();
    let source_bytes = generous.current_query_memory_bytes();
    let base = generous.reserve_query_memory(0).expect("tracker");
    // Act
    let (mut destination, destination_memory) =
        crate::runtime::owned_memory::try_reserve_vec(1, &base).expect("destination");
    destination.push(
        iterator
            .next_observing(&generous, || {})
            .expect("next")
            .expect("row"),
    );
    // Assert
    assert!(generous.current_query_memory_bytes() > source_bytes);
    drop(destination);
    drop(destination_memory);
    assert_eq!(generous.current_query_memory_bytes(), source_bytes);
    drop(iterator);
    assert_eq!(generous.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_exact_with_below_source_destination_overlap() {
    // Arrange
    let slot = std::mem::size_of::<BatchRow>();
    let required = 5 * slot;
    for (budget, succeeds) in [(required, true), (required - 1, false)] {
        let controls = controls(budget);
        let iterator = rows(&controls, 1, 4).into_iter();
        let base = controls.reserve_query_memory(0).expect("tracker");
        // Act
        let destination = crate::runtime::owned_memory::try_reserve_vec::<BatchRow>(1, &base);
        // Assert
        assert_eq!(destination.is_ok(), succeeds);
        drop(destination);
        assert_eq!(controls.current_query_memory_bytes(), 4 * slot);
        drop(iterator);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
