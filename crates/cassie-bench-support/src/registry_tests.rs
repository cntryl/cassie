use std::process::Command;

use super::{
    registered_targets, validate_cargo_metadata, validate_scenario_registry, BenchmarkTier,
    RunCategory, Target,
};
use crate::performance_benchmarks::benchmark_scenarios;

#[test]
fn should_reject_wrong_tier_target_ownership() {
    // Arrange
    let scenarios = benchmark_scenarios().copied().collect::<Vec<_>>();
    let scenario = scenarios[0];
    let wrong_tier = if scenario.declared_tier.number() == 1 {
        BenchmarkTier::Tier2
    } else {
        BenchmarkTier::Tier1
    };
    let targets = [Target::new(
        "cassie-e2e",
        scenario.benchmark,
        wrong_tier,
        RunCategory::Timed,
        &[],
    )];

    // Act
    let result = validate_scenario_registry(&targets, &[scenario]);

    // Assert
    assert!(result.unwrap_err().contains("wrong-tier"));
}

#[test]
fn should_reject_duplicate_scenario_ids() {
    // Arrange
    let scenario = *benchmark_scenarios()
        .next()
        .expect("scenario catalog is non-empty");
    let targets = [Target::new(
        "cassie-e2e",
        scenario.benchmark,
        BenchmarkTier::from_number(scenario.declared_tier.number())
            .expect("declared tier is valid"),
        RunCategory::Timed,
        &[],
    )];

    // Act
    let result = validate_scenario_registry(&targets, &[scenario, scenario]);

    // Assert
    assert!(result.unwrap_err().contains("duplicate scenario id"));
}

#[test]
fn should_reject_target_registration_with_missing_cargo_target() {
    // Arrange
    let targets = [Target::new(
        "cassie-core",
        "missing",
        BenchmarkTier::Tier1,
        RunCategory::Timed,
        &[],
    )];
    let metadata = r#"{"packages":[{"name":"cassie-core","targets":[]}]}"#;

    // Act
    let result = validate_cargo_metadata(&targets, metadata);

    // Assert
    assert!(result.unwrap_err().contains("missing"));
}

#[test]
fn should_reject_cargo_feature_placement_mismatch() {
    // Arrange
    let targets = [Target::new(
        "cassie-core",
        "tier1_memory",
        BenchmarkTier::Tier1,
        RunCategory::Timed,
        &["bench-feature"],
    )];
    let metadata = r#"{"packages":[{"name":"cassie-core","targets":[{"name":"tier1_memory","kind":["bench"],"required-features":[]}]}]}"#;

    // Act
    let result = validate_cargo_metadata(&targets, metadata);

    // Assert
    assert!(result.unwrap_err().contains("required features mismatch"));
}

#[test]
fn should_match_workspace_cargo_benchmark_targets() {
    // Arrange
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--locked", "--no-deps", "--format-version", "1"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run Cargo metadata for the benchmark workspace");
    assert!(output.status.success(), "Cargo metadata failed");
    let metadata = String::from_utf8(output.stdout).expect("Cargo metadata is UTF-8");

    // Act
    let result = validate_cargo_metadata(&registered_targets(), &metadata);

    // Assert
    assert_eq!(result, Ok(()));
}
