#[test]
fn should_register_paired_column_codec_acceptance_scenarios() {
    // Arrange
    let catalog = include_str!("../benches/support/performance_benchmark_catalog_tier2.rs");
    let expected = [
        "perf.column.selective_encoded_scan.2k",
        "selective_encoded_scan",
        "perf.column.selective_plain_scan_baseline.2k",
        "selective_plain_scan_baseline",
        "perf.column.incompressible_adaptive_scan.2k",
        "incompressible_adaptive_scan",
        "perf.column.incompressible_plain_scan_baseline.2k",
        "incompressible_plain_scan_baseline",
    ];

    // Act
    let registered = expected.map(|value| catalog.contains(value));
    let fixture_count = catalog.matches("2_048,\n        Tier2").count();

    // Assert
    assert!(registered.into_iter().all(|present| present));
    assert!(catalog.matches("\"tier2_subsystem_column_scan\"").count() >= 4);
    assert!(fixture_count >= 4);
}

#[test]
fn should_apply_column_codec_relative_p95_gates() {
    // Arrange
    let owner = include_str!("../benches/tier2_subsystem_column_scan.rs");
    let fixture = include_str!("../benches/support/workloads/column_codec_context.rs");

    // Act
    let relative_gate_count = owner.matches("require_relative_p95").count();
    let forces_plain_baseline = fixture
        .matches("rebuild_column_batches_plain_for_benchmark")
        .count();
    let verifies_incompressible_plain = fixture.contains("assert_plain_chunks");

    // Assert
    assert_eq!(relative_gate_count, 2);
    assert_eq!(forces_plain_baseline, 2);
    assert!(verifies_incompressible_plain);
}
