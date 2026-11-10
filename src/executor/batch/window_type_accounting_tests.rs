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
fn should_match_type_metadata_count_for_shared_distinct_with_empty_buffers() {
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
        row(Arc::new(vec![DataType::BigInt])),
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
fn should_admit_type_directory_at_the_exact_budget_with_release_it() {
    // Arrange
    let rows = vec![
        row(Arc::new(vec![DataType::Int])),
        row(Arc::new(vec![DataType::BigInt])),
    ];
    let directory_bytes = rows.len() * std::mem::size_of::<&Vec<DataType>>();

    // Act / Assert
    let retained_bytes = 7;
    for (budget, succeeds) in [
        (retained_bytes + directory_bytes, true),
        (retained_bytes + directory_bytes - 1, false),
    ] {
        let controls = controls(budget);
        let retained = controls
            .reserve_query_memory(retained_bytes)
            .expect("existing owner");
        let result = row_type_bytes_controlled(&rows, &controls);
        // Assert
        assert_eq!(result.is_ok(), succeeds);
        assert_eq!(controls.current_query_memory_bytes(), retained_bytes);
        drop(retained);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_reject_type_vector_size_overflow() {
    // Arrange
    let capacity = usize::MAX;

    // Act
    let result = window_type_accounting::vector_bytes(capacity);

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
}

#[test]
fn should_order_type_buffer_identities_for_odd_even_with_duplicate_inputs() {
    // Arrange
    let first = vec![DataType::Int];
    let second = vec![DataType::Text];
    let third = vec![DataType::BigInt];
    let controls = controls(1_000_000);
    let cases = [
        vec![&third, &first, &second],
        vec![&second, &first, &third, &first],
        vec![&third, &second],
    ];

    // Act / Assert
    for mut values in cases {
        window_type_accounting::sort(&mut values, &controls, &mut |_| {}).expect("sort");
        // Assert
        assert!(values
            .windows(2)
            .all(|pair| { (pair[0].as_ptr() as usize) <= (pair[1].as_ptr() as usize) }));
    }
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_actual_type_collection_heap_sift_with_array_walk() {
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
