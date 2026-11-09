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

fn null_output(controls: &QueryExecutionControls, count: usize) -> AggregateOutput {
    let source = controls.reserve_query_memory(0).expect("source");
    null_array_output(count, &source, controls, |_| {}).expect("null array output")
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

#[test]
fn should_cancel_selected_null_array_during_actual_alias_value_copy() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let result = null_output(&controls, 3);
    let mut output = row(&controls);
    let mut reached = false;

    // Act
    let status = output.append_with_progress(result, &["a".to_owned()], || {}, &mut |progress| {
        if matches!(progress, OutputProgress::ValueCopy) {
            reached = true;
            cancellation.cancel();
        }
    });

    // Assert
    assert!(reached);
    assert!(matches!(
        status,
        Err(QueryError::Cassie(CassieError::QueryCancelled))
    ));
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_selected_null_array_after_partial_alias_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let result = null_output(&controls, 2);
    let mut output = row(&controls);
    let mut copies = 0;

    // Act
    let status = output.append_with_observer(result, &["a".into(), "b".into()], || {
        copies += 1;
        cancellation.cancel();
    });

    // Assert
    assert_eq!(copies, 1);
    assert!(matches!(status, Err(QueryError::General(ref message)) if message == "query canceled"));
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_a_null_array_output_from_another_query_tracker() {
    // Arrange
    let first =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let second =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let result = null_output(&first, 1);
    let mut output = row(&second);

    // Act
    let status = output.append(result, &["a".to_owned()]);

    // Assert
    assert!(matches!(
        status,
        Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
    ));
    assert_eq!(first.current_query_memory_bytes(), 0);
    drop(output);
    assert_eq!(second.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_null_array_alias_copy_budget_at_exact_peak() {
    // Arrange
    let names = ["a".to_owned(), "b".to_owned()];
    let make_output = |controls: &QueryExecutionControls| null_output(controls, 2);
    let baseline =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let mut measured = row(&baseline);
    measured
        .append(make_output(&baseline), &names)
        .expect("measure");
    let peak = baseline.peak_query_memory_bytes();
    drop(measured);

    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: budget,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let mut output = row(&controls);
        let result = output.append(make_output(&controls), &names);
        assert_eq!(result.is_ok(), succeeds);
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    assert_eq!(baseline.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_a_forged_nonnull_array_before_alias_publication() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let memory = controls
        .reserve_query_memory(2 * std::mem::size_of::<serde_json::Value>())
        .expect("owner");
    let forged = AggregateOutput {
        value: Value::Json(serde_json::json!([null, 1])),
        memory: Some(memory),
        copy_mode: OutputCopyMode::NullArray,
    };
    let mut output = row(&controls);

    // Act
    let result = output.append(forged, &["a".to_owned()]);

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
    ));
    assert!(output.values.is_empty());
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
