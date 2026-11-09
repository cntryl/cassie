use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;
use crate::types::Value;
use std::time::Instant;

fn controls(budget: usize, cancel: QueryCancellationHandle) -> QueryExecutionControls {
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: budget,
        query_timeout_ms: 0,
        ..CassieRuntimeLimits::default()
    };
    QueryExecutionControls::with_cancellation(&limits, Instant::now(), cancel)
}

fn row() -> BatchRow {
    BatchRow::from_projected_values(vec![("v".into(), Value::String("value".into()))])
}

#[test]
fn should_keep_combined_group_lease_through_the_last_output_clone() {
    // Arrange
    let controls = controls(8192, QueryCancellationHandle::new());
    let mut group = GroupMemory::new(&controls).expect("group");
    group.add(100).expect("group charge");
    let mut partition = GroupMemory::new(&controls).expect("partition");
    partition.add(200).expect("partition charge");
    let existing = Arc::new(controls.reserve_query_memory(50).expect("row query"));
    let input = row().with_query_memory(Some(existing));
    // Act
    let mut output =
        retain_outputs_observing(vec![input], group, vec![partition], &controls, |_| {})
            .expect("output");
    let clone = output[0].clone();
    drop(output.pop());
    drop(output);
    // Assert
    assert!(clone.query_memory().is_some());
    assert!(clone.operator_memory().is_some());
    assert!(controls.current_query_memory_bytes() >= 350);
    drop(clone);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_mismatched_tracker_without_losing_either_reservation() {
    // Arrange
    let left = controls(8192, QueryCancellationHandle::new());
    let right = controls(8192, QueryCancellationHandle::new());
    let mut group = GroupMemory::new(&left).expect("group");
    group.add(100).expect("charge");
    let mut partition = GroupMemory::new(&right).expect("partition");
    partition.add(200).expect("charge");
    // Act
    let result = retain_outputs_observing(vec![row()], group, vec![partition], &left, |_| {});
    // Assert
    assert!(result.is_err());
    assert_eq!(left.current_query_memory_bytes(), 0);
    assert_eq!(right.current_query_memory_bytes(), 0);

    let group = GroupMemory::new(&right).expect("foreign primary");
    let result = retain_outputs_observing(vec![row()], group, Vec::new(), &left, |_| {});
    assert!(result.is_err());
    assert_eq!(left.current_query_memory_bytes(), 0);
    assert_eq!(right.current_query_memory_bytes(), 0);
}

#[test]
fn should_charge_header_and_link_at_exact_budget_and_release_after_partial_attach_cancel() {
    // Arrange
    let header = std::mem::size_of::<QueryMemoryReservation>() + 2 * std::mem::size_of::<usize>();
    let link = std::mem::size_of::<crate::executor::batch::OperatorMemory>()
        + 2 * std::mem::size_of::<usize>();
    for (budget, succeeds) in [(128 + header + link, true), (127 + header + link, false)] {
        let controls = controls(budget, QueryCancellationHandle::new());
        let mut group = GroupMemory::new(&controls).expect("group");
        group.add(128).expect("group charge");
        // Act
        let result = retain_outputs_observing(vec![row()], group, Vec::new(), &controls, |_| {});
        // Assert
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    let cancel = QueryCancellationHandle::new();
    let controls = controls(8192, cancel.clone());
    let mut group = GroupMemory::new(&controls).expect("group");
    group.add(128).expect("group charge");
    let mut attached = 0;
    let result = retain_outputs_observing(vec![row(), row()], group, Vec::new(), &controls, |_| {
        attached += 1;
        cancel.cancel();
    });
    assert_eq!(attached, 1);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_both_operator_links_when_output_already_has_a_query_owner() {
    // Arrange
    let header = std::mem::size_of::<QueryMemoryReservation>() + 2 * std::mem::size_of::<usize>();
    let link = std::mem::size_of::<crate::executor::batch::OperatorMemory>()
        + 2 * std::mem::size_of::<usize>();
    for (budget, succeeds) in [
        (150 + header + 2 * link, true),
        (149 + header + 2 * link, false),
    ] {
        let controls = controls(budget, QueryCancellationHandle::new());
        let mut group = GroupMemory::new(&controls).expect("group");
        group.add(100).expect("group charge");
        let query = Arc::new(controls.reserve_query_memory(50).expect("query owner"));
        let input = row().with_query_memory(Some(query));
        // Act
        let result = retain_outputs_observing(vec![input], group, Vec::new(), &controls, |_| {});
        // Assert
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_group_lease_without_allocating_links_for_empty_output() {
    // Arrange
    let controls = controls(128, QueryCancellationHandle::new());
    let mut group = GroupMemory::new(&controls).expect("group");
    group.add(128).expect("charge");
    // Act
    let output =
        retain_outputs_observing(Vec::new(), group, Vec::new(), &controls, |_| {}).expect("empty");
    // Assert
    assert!(output.is_empty());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
