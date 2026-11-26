use cassie_bench_support::performance_benchmarks::benchmark_scenarios;
use cassie_bench_support::registry::{
    select_targets, validate_scenario_registry, BenchmarkTier, RunCategory, Target, TargetSelection,
};

#[test]
fn should_select_only_registry_targets_in_the_requested_scope() {
    // Arrange
    let targets = [
        Target::new(
            "cassie-core",
            "tier1_memory",
            BenchmarkTier::Tier1,
            RunCategory::Timed,
            &[],
        ),
        Target::new(
            "cassie-e2e",
            "tier5_query",
            BenchmarkTier::Tier5,
            RunCategory::Timed,
            &[],
        ),
        Target::new(
            "cassie-e2e",
            "relevance",
            BenchmarkTier::Tier3,
            RunCategory::Diagnostic,
            &[],
        ),
    ];
    let selection = TargetSelection {
        tiers: vec![BenchmarkTier::Tier1, BenchmarkTier::Tier5],
        categories: vec![RunCategory::Timed],
        targets: vec!["tier5_query".to_owned()],
    };

    // Act
    let selected = select_targets(&targets, &selection);

    // Assert
    assert_eq!(
        selected
            .iter()
            .map(|target| target.target)
            .collect::<Vec<_>>(),
        ["tier5_query"]
    );
}

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
    let result = cassie_bench_support::registry::validate_cargo_metadata(&targets, metadata);

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
    let result = cassie_bench_support::registry::validate_cargo_metadata(&targets, metadata);

    // Assert
    assert!(result.unwrap_err().contains("required features mismatch"));
}
