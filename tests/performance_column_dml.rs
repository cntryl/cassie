#[test]
fn should_register_tier_five_column_dml_amplification_curves() {
    // Arrange
    let catalog = include_str!("../benches/support/performance_benchmark_catalog_tier5.rs");
    let expected = [
        "perf.scale.query.column_dml.10k",
        "perf.scale.query.column_dml.100k",
        "perf.scale.query.column_dml.250k",
    ];

    // Act
    let registered = expected.map(|scenario_id| catalog.contains(scenario_id));
    let workload_count = catalog.matches("\"column_dml\"").count();

    // Assert
    assert!(registered.into_iter().all(|present| present));
    assert_eq!(workload_count, 3);
    assert!(catalog.contains("10_000,\n        Tier5"));
    assert!(catalog.contains("100_000,\n        Tier5"));
    assert!(catalog.contains("250_000,\n        Tier5"));
}

#[test]
fn should_measure_column_dml_with_write_amplification_evidence() {
    // Arrange
    let workload = include_str!("../benches/support/workloads/scaling.rs");
    let owner = include_str!("../benches/tier5_scaling_query.rs");

    // Act
    let records_rewrites = workload.contains("\"segment_rewrites\"");
    let records_source_rows = workload.contains("\"maintenance_source_rows\"");
    let exercises_encoded_filter =
        workload.contains("WHERE status = 'approved' AND score >= 90 LIMIT 1000");
    let invokes_workload = owner.contains("workloads::column_dml");

    // Assert
    assert!(records_rewrites);
    assert!(records_source_rows);
    assert!(exercises_encoded_filter);
    assert!(invokes_workload);
}
