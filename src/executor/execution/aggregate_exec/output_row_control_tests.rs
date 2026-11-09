//! Owned aggregate output directory and direct String-copy probes.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;
use std::time::Instant;

fn row(controls: &QueryExecutionControls) -> OutputRow {
    OutputRow {
        values: Vec::new(),
        memory: Some(controls.reserve_query_memory(0).expect("owner")),
        controls: controls.clone(),
    }
}

#[test]
fn should_release_owned_output_after_cancellation_during_directory_move() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut output = row(&controls);
    let names = ["a".to_owned(), "b".to_owned()];
    output
        .append(AggregateOutput::from_value(Value::Int64(1)), &names)
        .expect("initial values");
    assert_eq!(output.values.len(), output.values.capacity());
    let mut moved = false;

    // Act
    let result = push_output_observing(
        &mut output.values,
        &mut output.memory,
        &output.controls,
        "c",
        &Value::Int64(2),
        &mut |progress| {
            if matches!(progress, OutputProgress::DirectoryMove) {
                moved = true;
                cancellation.cancel();
            }
        },
    );

    // Assert
    assert!(moved);
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::QueryCancelled))
    ));
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_owned_output_after_cancellation_during_string_copy() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut output = row(&controls);
    let value = Value::String("λ".repeat(8192));
    let mut copied = false;

    // Act
    let result = push_output_observing(
        &mut output.values,
        &mut output.memory,
        &output.controls,
        "alias",
        &value,
        &mut |progress| {
            if matches!(progress, OutputProgress::ValueCopy) {
                copied = true;
                cancellation.cancel();
            }
        },
    );

    // Assert
    assert!(copied);
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::QueryCancelled))
    ));
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_duplicate_names_and_enforce_output_budget_boundary() {
    // Arrange
    let value = Value::String("escaped \"Ω".repeat(256));
    let names = ["alias".to_owned(), "alias".to_owned()];
    let baseline =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let mut measured = row(&baseline);
    measured
        .append(AggregateOutput::from_value(value.clone()), &names)
        .expect("measure");
    let required = baseline.peak_query_memory_bytes();
    drop(measured);
    assert!(required > 0);

    for (budget, accepted) in [(required - 1, false), (required, true)] {
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: budget,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let mut output = row(&controls);
        // Act
        let result = output.append(AggregateOutput::from_value(value.clone()), &names);
        // Assert
        assert_eq!(result.is_ok(), accepted);
        if accepted {
            assert_eq!(
                output.values,
                vec![
                    ("alias".to_owned(), value.clone()),
                    ("alias".to_owned(), value.clone())
                ]
            );
        }
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
