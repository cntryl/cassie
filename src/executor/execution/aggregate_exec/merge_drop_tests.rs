use super::*;
use crate::runtime::QueryCancellationHandle;

#[test]
fn should_release_merge_reservations_after_cancellation_at_occupied_mutation() {
    // Arrange
    let plan = aggregate_plan("SELECT tenant, COUNT(*) FROM events GROUP BY tenant");
    let specs = aggregate_specs(&plan);
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let functions = HashMap::new();
    let context = aggregate_context(&plan, &functions, &controls);
    let rows = [tenant_row(0), tenant_row(0)];
    let left = aggregate_partition(&rows[..1], &specs, &context).expect("left partition");
    let right = aggregate_partition(&rows[1..], &specs, &context).expect("right partition");
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("fixture");
    MERGED_GROUP_MUTATION_CANCEL.with(|slot| *slot.borrow_mut() = Some(cancellation));
    // Act
    let result = merge_partial_aggregations(&cassie, vec![left, right], 2, &rows, &specs, &context);
    // Assert
    assert!(matches!(result, Err(QueryError::General(ref message)) if message == "query canceled"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
