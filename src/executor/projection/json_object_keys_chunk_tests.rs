//! Deferred guarded object-key chunk handoff probes.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
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
fn fixture(count: usize) -> (Vec<Batch>, Vec<SelectItem>) {
    let parsed = crate::sql::parse_statement("SELECT json_object_keys(doc) AS keys FROM records")
        .expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };
    let keys = (0..count)
        .map(|index| serde_json::Value::String(index.to_string()))
        .collect();
    (
        vec![vec![BatchRow::new(vec![(
            "keys".into(),
            Value::Json(serde_json::Value::Array(keys)),
        )])]],
        select.projection,
    )
}

#[test]
fn should_preserve_empty_boundary_with_multiple_chunk_order() {
    // Arrange
    for count in [0, 1, 1024, 1025, 2051] {
        let controls = controls(64 * 1024 * 1024);
        let (batches, projection) = fixture(count);
        // Act
        let expanded =
            json_object_keys::expand(batches, &projection, &controls).expect("expansion");
        // Assert
        let lengths: Vec<_> = expanded.iter().map(Vec::len).collect();
        let expected: Vec<_> = (0..count)
            .step_by(1024)
            .map(|start| (count - start).min(1024))
            .collect();
        assert_eq!(lengths, expected);
        for (index, row) in expanded.iter().flatten().enumerate() {
            assert_eq!(row.entries()[0].1, Value::String(index.to_string()));
        }
        drop(expanded);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_hold_chunk_directories_after_rows_are_drained_until_guard_drops() {
    // Arrange
    let controls = controls(16 * 1024 * 1024);
    let (batches, projection) = fixture(2);
    let mut expanded =
        json_object_keys::expand(batches, &projection, &controls).expect("expansion");
    // Act
    expanded.batches_mut()[0].clear();
    // Assert
    assert!(expanded.chunk_bytes() > 0);
    assert!(controls.current_query_memory_bytes() >= expanded.chunk_bytes());
    drop(expanded);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_chunk_with_source_owners_after_actual_directory_or_move_cancellation() {
    // Arrange
    for stage in [6, 7] {
        let cancel = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancel.clone(),
        );
        let (batches, projection) = fixture(3);
        json_object_keys::cancel_expand_once(cancel.clone(), stage);
        // Act
        let result = json_object_keys::expand(batches, &projection, &controls);
        // Assert
        assert!(cancel.is_canceled(), "stage {stage}");
        assert!(result.is_err(), "stage {stage}");
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_owned_batches_on_flatten_boundary_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let (batches, projection) = fixture(2);
    let expanded = json_object_keys::expand(batches, &projection, &controls).expect("expansion");
    json_object_keys::cancel_expand_once(cancel.clone(), 8);
    // Act
    let result = expanded.flatten(&controls);
    // Assert
    assert!(cancel.is_canceled());
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_chunk_owner_after_error_in_consuming_transform() {
    // Arrange
    let controls = controls(16 * 1024 * 1024);
    let (batches, projection) = fixture(2);
    let mut expanded =
        json_object_keys::expand(batches, &projection, &controls).expect("expansion");
    let bytes = expanded.chunk_bytes();
    // Act
    let result = expanded.transform(|batches| {
        drop(batches);
        Err(QueryError::General("injected distinct failure".into()))
    });
    // Assert
    assert!(result.is_err());
    assert!(bytes > 0);
    assert!(controls.current_query_memory_bytes() >= bytes);
    drop(expanded);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_unselected_batches_untracked_by_chunk_guard() {
    // Arrange
    let controls = controls(16 * 1024 * 1024);
    let parsed = crate::sql::parse_statement("SELECT doc FROM records").expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };
    let batches = vec![vec![BatchRow::new(vec![("doc".into(), Value::Int64(1))])]];
    // Act
    let expanded =
        json_object_keys::expand(batches, &select.projection, &controls).expect("unselected");
    // Assert
    assert_eq!(expanded.chunk_bytes(), 0);
    assert_eq!(expanded[0][0].entries()[0].1, Value::Int64(1));
    drop(expanded);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_measured_chunk_handoff_peak_with_release_below_budget() {
    // Arrange
    let generous = controls(64 * 1024 * 1024);
    let (batches, projection) = fixture(1025);
    let guard = json_object_keys::expand(batches, &projection, &generous).expect("expansion");
    let peak = generous.peak_query_memory_bytes();
    assert!(guard.chunk_bytes() > 0);
    drop(guard);
    assert_eq!(generous.current_query_memory_bytes(), 0);
    // Act
    let exact = controls(peak);
    let (batches, projection) = fixture(1025);
    let guard = json_object_keys::expand(batches, &projection, &exact).expect("exact peak");
    drop(guard);
    let below = controls(peak - 1);
    let (batches, projection) = fixture(1025);
    let result = json_object_keys::expand(batches, &projection, &below);
    // Assert
    assert!(result.is_err());
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert_eq!(below.current_query_memory_bytes(), 0);
}

#[test]
fn should_record_one_batch_buffer_metric_for_each_emitted_chunk() {
    // Arrange
    let controls = controls(64 * 1024 * 1024);
    let (batches, projection) = fixture(2051);
    let _ = crate::executor::batch::take_local_batch_buffers();
    // Act
    let expanded = json_object_keys::expand(batches, &projection, &controls).expect("expansion");
    let observed = crate::executor::batch::take_local_batch_buffers();
    // Assert
    assert_eq!(observed, expanded.len() as u64);
    assert_eq!(observed, 3);
    drop(expanded);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_both_directory_leases_after_actual_flatten_move_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let (batches, projection) = fixture(2);
    let expanded = json_object_keys::expand(batches, &projection, &controls).expect("expansion");
    json_object_keys::cancel_expand_once(cancel.clone(), 9);
    // Act
    let result = expanded.flatten(&controls);
    // Assert
    assert!(cancel.is_canceled());
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_measured_flatten_overlap_with_release_on_below_peak() {
    // Arrange
    let generous = controls(64 * 1024 * 1024);
    let (batches, projection) = fixture(1025);
    let expanded = json_object_keys::expand(batches, &projection, &generous).expect("expansion");
    let rows = expanded.flatten(&generous).expect("flatten");
    let peak = generous.peak_query_memory_bytes();
    drop(rows);
    // Assert
    assert_eq!(generous.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let limited = controls(budget);
        let (batches, projection) = fixture(1025);
        let result = json_object_keys::expand(batches, &projection, &limited)
            .and_then(|expanded| expanded.flatten(&limited));
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(limited.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_hold_chunk_reservation_through_partial_pull_iterator_drop() {
    // Arrange
    let controls = controls(16 * 1024 * 1024);
    let (batches, projection) = fixture(1025);
    let expanded = json_object_keys::expand(batches, &projection, &controls).expect("expansion");
    let bytes = expanded.chunk_bytes();
    let mut iterator = expanded.into_rows();
    // Act
    let row = iterator.next().expect("first row");
    // Assert
    assert!(bytes > 0);
    assert!(controls.current_query_memory_bytes() >= bytes);
    drop(row);
    drop(iterator);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_multibatch_pull_order_with_release_after_exhaustion() {
    // Arrange
    let controls = controls(16 * 1024 * 1024);
    let (batches, projection) = fixture(2051);
    let expanded = json_object_keys::expand(batches, &projection, &controls).expect("expansion");
    let bytes = expanded.chunk_bytes();
    let mut iterator = expanded.into_rows();
    // Act
    for index in 0..2051 {
        let row = iterator.next().expect("row");
        assert_eq!(row.entries()[0].1, Value::String(index.to_string()));
    }
    // Assert
    assert!(iterator.next().is_none());
    assert!(controls.current_query_memory_bytes() >= bytes);
    drop(iterator);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_iterate_untracked_empty_with_nonempty_batches_in_order() {
    // Arrange
    let first = BatchRow::new(vec![("k".into(), Value::Int64(1))]);
    let second = BatchRow::new(vec![("k".into(), Value::Int64(2))]);
    let batches = vec![Vec::new(), vec![first], Vec::new(), vec![second]];
    // Act
    let mut rows = OwnedExpanded::untracked(batches).into_rows();
    // Assert
    assert_eq!(rows.next().expect("first").entries()[0].1, Value::Int64(1));
    assert_eq!(rows.next().expect("second").entries()[0].1, Value::Int64(2));
    assert!(rows.next().is_none());
}

#[test]
fn should_not_record_additional_batch_buffers_when_pull_iterator_consumes_chunks() {
    // Arrange
    let controls = controls(16 * 1024 * 1024);
    let (batches, projection) = fixture(2051);
    let _ = crate::executor::batch::take_local_batch_buffers();
    let expanded = json_object_keys::expand(batches, &projection, &controls).expect("expansion");
    assert_eq!(crate::executor::batch::take_local_batch_buffers(), 3);
    // Act
    let mut rows = expanded.into_rows();
    while rows.next().is_some() {}
    // Assert
    assert_eq!(crate::executor::batch::take_local_batch_buffers(), 0);
    drop(rows);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
