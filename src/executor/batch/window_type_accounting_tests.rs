use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
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

fn row(types: Arc<Vec<DataType>>) -> BatchRow {
    BatchRow::new(Vec::new()).with_optional_data_types(Some(types))
}

#[test]
fn should_match_type_metadata_count_for_shared_distinct_and_empty_buffers() {
    // Arrange
    let shared = Arc::new(vec![DataType::Array(Box::new(DataType::Array(Box::new(
        DataType::Int,
    ))))]);
    let mut extra_capacity = Vec::with_capacity(8);
    extra_capacity.push(DataType::BigInt);
    let rows = vec![
        row(Arc::clone(&shared)),
        row(shared),
        row(Arc::new(extra_capacity)),
        row(Arc::new(Vec::new())),
        row(Arc::new(Vec::new())),
        BatchRow::new(Vec::new()),
    ];
    let controls = controls(1_000_000);

    // Act
    let actual = row_type_bytes_controlled(&rows, &controls).expect("count metadata");

    // Assert
    assert_eq!(actual, row_type_bytes(&rows));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_type_directory_at_the_exact_budget_and_release_it() {
    // Arrange
    let rows = vec![
        row(Arc::new(vec![DataType::Int])),
        row(Arc::new(vec![DataType::BigInt])),
    ];
    let directory_bytes = rows.len() * std::mem::size_of::<&Vec<DataType>>();

    // Act / Assert
    for (budget, succeeds) in [(directory_bytes, true), (directory_bytes - 1, false)] {
        let controls = controls(budget);
        let result = row_type_bytes_controlled(&rows, &controls);
        assert_eq!(result.is_ok(), succeeds);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_cancel_actual_type_collection_heap_sift_and_array_walk() {
    // Arrange
    let rows = vec![
        row(Arc::new(vec![DataType::Array(Box::new(DataType::Array(
            Box::new(DataType::Int),
        )))])),
        row(Arc::new(vec![DataType::BigInt])),
        row(Arc::new(vec![DataType::Text])),
    ];
    for boundary in [
        TypeCountProgress::Collect,
        TypeCountProgress::Sift,
        TypeCountProgress::Array,
    ] {
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        let mut reached = false;

        // Act
        let result = row_type_bytes_controlled_observed(&rows, &controls, |progress| {
            if progress == boundary {
                reached = true;
                cancellation.cancel();
            }
        });

        // Assert
        assert!(reached, "boundary {boundary:?} was not reached");
        assert!(matches!(result, Err(CassieError::QueryCancelled)));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
