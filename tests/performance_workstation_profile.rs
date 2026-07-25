const PROFILE_ID: &str = "workstation-apple-m5-arm64-apfs";

#[test]
fn should_register_the_named_apple_m5_evidence_profile() {
    // Arrange
    let profiles = include_str!("../benches/support/performance_benchmark_profiles.rs");

    // Act
    let has_profile = profiles.contains(PROFILE_ID);

    // Assert
    assert!(has_profile);
    assert!(profiles.contains("Apple M5 workstation, arm64, APFS"));
    assert!(profiles.contains("storage_mode: \"midge_disk_apfs\""));
    assert!(profiles.contains("fixture_scale: \"10k+100k+250k\""));
    assert!(profiles.contains("\"not_native_linux\""));
    assert!(profiles.contains("deployment_profile_for_id"));
}

#[test]
fn should_make_the_named_workstation_profile_disk_backed() {
    // Arrange
    let harness = include_str!("../benches/support/stress.rs");

    // Act
    let selects_disk = harness.contains("profile.storage_mode == \"midge_disk_apfs\"");
    let configures_midge = harness.contains("std::env::set_var(\"BENCH_MIDGE_DISK\", \"1\")");

    // Assert
    assert!(selects_disk);
    assert!(configures_midge);
}
