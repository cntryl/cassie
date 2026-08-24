const NATIVE_LINUX_PROFILE_ID: &str = "native-linux-amd64-disk";

#[test]
fn should_allow_six_hours_for_the_complete_canonical_benchmark_suite() {
    // Arrange
    let workflow = include_str!("../.github/workflows/benchmarks.yml");

    // Act
    let allows_complete_suite = workflow.contains("timeout-minutes: 360");

    // Assert
    assert!(allows_complete_suite);
}

#[test]
fn should_register_the_workflow_native_linux_profile() {
    // Arrange
    let workflow = include_str!("../.github/workflows/benchmarks.yml");
    let documentation = include_str!("../docs/deployment-profiles.md");
    let profiles = include_str!("../benches/support/performance_benchmark_profiles.rs");

    // Act
    let workflow_accepts_a_profile = workflow.contains("deployment_profile:");
    let documented = documentation.contains(NATIVE_LINUX_PROFILE_ID);
    let registered = profiles.contains(NATIVE_LINUX_PROFILE_ID);

    // Assert
    assert!(workflow_accepts_a_profile);
    assert!(documented);
    assert!(registered);
    assert!(profiles.contains("storage_mode: \"midge_disk_native_linux\""));
}

#[test]
fn should_select_local_storage_for_native_linux_disk_evidence() {
    // Arrange
    let harness = include_str!("../benches/support/stress.rs");
    let workload_context = include_str!("../benches/support/workloads/context.rs");

    // Act
    let recognizes_native_linux_disk = harness.contains("\"midge_disk_native_linux\"");
    let configures_local_storage =
        harness.contains("std::env::set_var(\"CASSIE_STORAGE_MODE\", \"local\")");
    let preserves_profile_storage =
        workload_context.contains("var_os(\"CASSIE_BENCH_DEPLOYMENT_PROFILE_ID\").is_none()");

    // Assert
    assert!(recognizes_native_linux_disk);
    assert!(configures_local_storage);
    assert!(preserves_profile_storage);
}
