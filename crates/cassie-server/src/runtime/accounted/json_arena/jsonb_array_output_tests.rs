use std::time::Instant;

use super::{render, render_observed, OutputError, Progress};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn input(value: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for header in [
        1_i32,
        0,
        3802,
        1,
        1,
        i32::try_from(value.len()).expect("length"),
    ] {
        bytes.extend_from_slice(&header.to_be_bytes());
    }
    bytes.extend_from_slice(value);
    bytes
}

#[test]
fn should_admit_exact_peak_with_release_array_output_owners() {
    // Arrange
    let bytes = input(b"\x01{\"x\":1,\"y\":[2,3]}");
    let generous =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let result = render(&bytes, 1, &generous).expect("measure");
    let peak = generous.peak_query_memory_bytes();
    drop(result);
    let limits = |budget| CassieRuntimeLimits {
        query_memory_budget_bytes: budget,
        ..CassieRuntimeLimits::default()
    };
    let exact = QueryExecutionControls::from_limits(&limits(peak), Instant::now());
    let below = QueryExecutionControls::from_limits(&limits(peak - 1), Instant::now());
    // Act / Assert
    let output = render(&bytes, 1, &exact).expect("exact");
    // Assert
    assert!(matches!(
        render(&bytes, 1, &below),
        Err(OutputError::Control(CassieError::ResourceLimit(_)))
    ));
    drop(output);
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert_eq!(below.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_after_actual_array_element_validation_with_during_output_copy() {
    for target in [0, 1] {
        // Arrange
        let bytes = input(b"\x01{\"x\":[1,2]}");
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        let mut observed = false;
        // Act
        let result = render_observed(&bytes, 1, &controls, |progress| {
            if matches!(
                (target, progress),
                (0, Progress::ValidatedElement) | (1, Progress::OutputCopy)
            ) {
                observed = true;
                cancellation.cancel();
            }
        });
        // Assert
        assert!(observed);
        assert!(matches!(
            result,
            Err(OutputError::Control(CassieError::QueryCanceled))
        ));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
