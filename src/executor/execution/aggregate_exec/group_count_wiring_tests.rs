use super::*;
use crate::runtime::QueryCancellationHandle;

#[test]
fn should_preserve_duplicate_with_empty_group_results_at_measured_budget() {
    // Arrange
    let plan = aggregate_plan("SELECT tenant, COUNT(*) FROM events GROUP BY tenant");
    let specs = aggregate_specs(&plan);
    let functions = HashMap::new();
    let baseline = controls_with_budget(1_000_000);
    let context = aggregate_context(&plan, &functions, &baseline);
    let rows = vec![tenant_row(0), tenant_row(0), tenant_row(1)];
    // Act
    let measured = aggregate_query_batches_serial(rows, &specs, &context).expect("groups");
    let peak = baseline.peak_query_memory_bytes();
    // Assert
    assert_eq!(measured.iter().flatten().count(), 2);
    drop(measured);
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = controls_with_budget(budget);
        let context = aggregate_context(&plan, &functions, &controls);
        let result = aggregate_query_batches_serial(
            vec![tenant_row(0), tenant_row(0), tenant_row(1)],
            &specs,
            &context,
        );
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    let empty = aggregate_query_batches_serial(Vec::new(), &specs, &context).expect("empty groups");
    assert!(empty.is_empty());
    drop(empty);
    assert_eq!(baseline.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_partial_groups_after_actual_next_group_count_cancellation() {
    // Arrange
    let plan = aggregate_plan("SELECT tenant, COUNT(*) FROM events GROUP BY tenant");
    let specs = aggregate_specs(&plan);
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let functions = HashMap::new();
    let mut context = aggregate_context(&plan, &functions, &controls);
    let arm = |processed| {
        if processed == 1 {
            group_size::cancel_next_chunk(cancel.clone());
        }
    };
    context.after_partition_row = Some(&arm);
    // Act
    let second = BatchRow::new(vec![("tenant".into(), Value::String("long".repeat(300)))]);
    let result = aggregate_partition(&[tenant_row(0), second], &specs, &context);
    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
