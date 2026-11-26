use std::collections::BTreeSet;

use cassie_bench_support::artifacts::{
    expected_scenario_cases, validate_run_manifest, RunManifest, RunScope,
};
use cassie_bench_support::registry::{registered_targets, BenchmarkTier, RunCategory};

#[test]
fn should_require_twelve_time_series_full_axes() {
    // Arrange
    let scope = RunScope {
        tiers: vec![BenchmarkTier::Tier5],
        categories: vec![RunCategory::Timed],
        targets: vec!["tier5_time_series_mixed".to_owned()],
        profile: "native-linux-arm64-disk".to_owned(),
        run_profile: "full".to_owned(),
        require_full_soaks: false,
    };
    let targets = BTreeSet::from(["tier5_time_series_mixed".to_owned()]);

    // Act
    let cases = expected_scenario_cases(&scope, &targets);

    // Assert
    assert_eq!(cases.len(), 12);
    assert!(cases.keys().any(|case| case.ends_with(".100k.16devices")));
    assert!(cases.keys().any(|case| case.ends_with(".250k.1024devices")));
}

#[test]
fn should_separate_time_series_smoke_axes_from_full_fixture_ids() {
    // Arrange
    let scope = RunScope {
        tiers: vec![BenchmarkTier::Tier5],
        categories: vec![RunCategory::Timed],
        targets: vec!["tier5_time_series_mixed".to_owned()],
        profile: "native-linux-arm64-disk".to_owned(),
        run_profile: "smoke".to_owned(),
        require_full_soaks: false,
    };
    let targets = BTreeSet::from(["tier5_time_series_mixed".to_owned()]);

    // Act
    let cases = expected_scenario_cases(&scope, &targets);

    // Assert
    assert_eq!(cases.len(), 6);
    assert!(cases.keys().all(|case| case.contains(".smoke.")));
}

#[test]
fn should_reject_missing_expected_timed_scenarios() {
    // Arrange
    let manifest: RunManifest = serde_json::from_str(r#"{
      "schema_version":1,"suite_id":"tier1","run_id":"r1",
      "scope":{"tiers":[1],"categories":["timed"],"targets":[],"profile":"native-linux-arm64-disk","require_full_soaks":false},
      "provenance":{"commit":"abc","toolchain":"rustc","host_class":"linux-amd64","job_id":"j1"},
      "artifacts":[]
    }"#).unwrap();

    // Act
    let result = validate_run_manifest(&manifest, &registered_targets(), &Default::default());

    // Assert
    assert!(result.unwrap_err().contains("missing"));
}

#[test]
fn should_reject_non_timed_artifacts_in_a_timed_scope() {
    // Arrange
    let manifest: RunManifest = serde_json::from_str(r#"{
      "schema_version":1,"suite_id":"tier3","run_id":"r1",
      "scope":{"tiers":[3],"categories":["timed"],"targets":[],"profile":"native-linux-arm64-disk","require_full_soaks":false},
      "provenance":{"commit":"abc","toolchain":"rustc","host_class":"linux-amd64","job_id":"j1"},
      "artifacts":[{"package":"cassie-e2e","target":"relevance","tier":3,"category":"diagnostic","profile":"native","commit":"abc","toolchain":"rustc","host_class":"linux-amd64","job_id":"j1","elapsed_seconds":2,"raw_path":"raw.json"}]
    }"#).unwrap();

    // Act
    let result = validate_run_manifest(&manifest, &registered_targets(), &Default::default());

    // Assert
    assert!(result.unwrap_err().contains("category"));
}
