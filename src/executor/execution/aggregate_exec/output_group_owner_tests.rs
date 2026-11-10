use super::*;

#[test]
fn should_retain_serial_group_memory_through_output_lifetime() {
    // Arrange
    let plan = aggregate_plan("SELECT tenant, COUNT(*) FROM events GROUP BY tenant");
    let specs = aggregate_specs(&plan);
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let functions = HashMap::new();
    let context = aggregate_context(&plan, &functions, &controls);
    // Act
    let output = aggregate_query_batches_serial(vec![tenant_row(0)], &specs, &context)
        .expect("serial result");
    let row = output.iter().flatten().next().expect("row").clone();
    drop(output);
    // Assert
    assert!(row.operator_memory().is_some());
    assert!(controls.current_query_memory_bytes() > 0);
    drop(row);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_partition_with_merge_memory_through_output_lifetime() {
    // Arrange
    let plan = aggregate_plan("SELECT tenant, COUNT(*) FROM events GROUP BY tenant");
    let specs = aggregate_specs(&plan);
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let functions = HashMap::new();
    let context = aggregate_context(&plan, &functions, &controls);
    let rows = [tenant_row(0), tenant_row(0)];
    let left = aggregate_partition(&rows[..1], &specs, &context).expect("left");
    let right = aggregate_partition(&rows[1..], &specs, &context).expect("right");
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("fixture");
    // Act
    let output = merge_partial_aggregations(&cassie, vec![left, right], 2, &rows, &specs, &context)
        .expect("merge")
        .expect("parallel");
    let row = output.iter().flatten().next().expect("row").clone();
    drop(output);
    // Assert
    assert!(row.operator_memory().is_some());
    assert!(controls.current_query_memory_bytes() > 0);
    drop(row);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
