use super::*;
use crate::runtime::QueryCancellationHandle;

#[test]
fn should_release_serial_group_memory_after_insert_cancellation() {
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
    let context = aggregate_context(&plan, &functions, &controls);
    SERIAL_GROUP_MUTATION_CANCEL.with(|slot| *slot.borrow_mut() = Some(cancel));
    // Act
    let result = aggregate_query_batches_serial(vec![tenant_row(0)], &specs, &context);
    // Assert
    assert!(matches!(result, Err(QueryError::General(ref message)) if message == "query canceled"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_partial_group_memory_after_update_cancellation() {
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
    let context = aggregate_context(&plan, &functions, &controls);
    PARTIAL_GROUP_MUTATION_CANCEL.with(|slot| *slot.borrow_mut() = Some(cancel));
    // Act
    let result = aggregate_partition(&[tenant_row(0)], &specs, &context);
    // Assert
    assert!(matches!(result, Err(QueryError::General(ref message)) if message == "query canceled"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
