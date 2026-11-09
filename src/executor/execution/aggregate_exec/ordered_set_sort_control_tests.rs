//! Synchronized interruption at actual ordered-set sort work.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;
use std::time::Instant;

#[test]
fn should_cancel_actual_index_initialization_inverse_and_inner_swaps() {
    // Arrange
    for boundary in [
        SortProgress::Initialize,
        SortProgress::Inverse,
        SortProgress::Swap,
    ] {
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        let mut values = vec![2, 3, 4, 0, 1];
        let mut steps = 0;

        // Act
        let result = try_sort_by_observed(&mut values, &controls, i32::cmp, |progress| {
            if progress == boundary {
                steps += 1;
                if boundary != SortProgress::Swap || steps == 3 {
                    cancellation.cancel();
                }
            }
        });

        // Assert
        assert!(
            matches!(result, Err(QueryError::General(ref message)) if message == "query canceled")
        );
        assert_eq!(steps, if boundary == SortProgress::Swap { 3 } else { 1 });
        values.sort_unstable();
        assert_eq!(values, [0, 1, 2, 3, 4]);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_stable_order_when_checked_sort_completes() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let mut values = vec![(2, 'a'), (1, 'b'), (2, 'c'), (1, 'd')];
    let mut swaps = 0;

    // Act
    try_sort_by_observed(
        &mut values,
        &controls,
        |left, right| left.0.cmp(&right.0),
        |progress| {
            if progress == SortProgress::Swap {
                swaps += 1;
            }
        },
    )
    .expect("sort");

    // Assert
    assert_eq!(values, [(1, 'b'), (1, 'd'), (2, 'a'), (2, 'c')]);
    assert!(swaps > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_both_sort_buffers_before_initialization() {
    // Arrange
    let values = vec![3, 1, 2, 0];
    let baseline =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let mut measured = values.clone();
    try_sort_by_observed(&mut measured, &baseline, i32::cmp, |_| {}).expect("measure workspace");
    let peak = baseline.peak_query_memory_bytes();
    assert!(peak >= 2 * values.len() * size_of::<usize>());
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: budget,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let mut input = values.clone();
        let mut initialized = false;

        // Act
        let result = try_sort_by_observed(&mut input, &controls, i32::cmp, |progress| {
            if progress == SortProgress::Initialize {
                initialized = true;
            }
        });

        // Assert
        assert_eq!(result.is_ok(), succeeds);
        assert_eq!(initialized, succeeds);
        if !succeeds {
            assert_eq!(input, values);
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
