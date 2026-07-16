#[path = "../benches/support/performance_benchmarks.rs"]
pub mod performance_benchmarks;

use performance_benchmarks::{
    artifact_output_dir, benchmark_for_scenario, benchmark_scenarios,
    expected_complete_benchmark_manifest, validate_complete_benchmark_artifacts,
    validate_complete_benchmark_contract, validate_complete_benchmark_suite,
    BenchmarkOwnerManifest, BenchmarkSuiteManifest, BenchmarkTimingMode, ResultCachePolicy,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const TIER1_OWNER: &str = "tier1_hotpath_row_codec";
const TIER2_OWNER: &str = "tier2_subsystem_plan_cache";
const TIER3_VECTOR_OWNER: &str = "tier3_system_query";
const TIER5_OWNER: &str = "tier5_scaling_query";
const TIER6_OWNER: &str = "tier6_soak_mixed";
const TIER1_SCENARIO: &str = "perf.kernel.row_codec";
const TIER2_SCENARIO: &str = "perf.cache.plan_hit.1k";
const RESULT_CACHE_SCENARIO: &str = "perf.cache.result_hit.1k";
const TIER3_VECTOR_SCENARIO: &str = "perf.vector.hnsw_persisted.100k";
const TIER5_SCENARIO: &str = "perf.scale.query.relational.10k";
const TIER6_SCENARIO: &str = "perf.soak.mixed.100k";
const SECOND: u64 = 1_000_000_000;

fn artifact(overrides: &str) -> String {
    format!(
        r#"{{
            "schema_version": "cntryl-stress.v2",
            "run_profile": "default",
            "metadata": {{ "filtered_run": "false" }},
            "environment": {{
                "git_commit": "expected-commit",
                "rustc_version": "expected-toolchain"
            }},
            "summaries": [{{
                "tier": 3,
                "metadata": {{
                    "scenario_id": "perf.query.10k",
                    "benchmark": "tier3_system_query",
                    "workload": "simple_sql_query",
                    "fixture_scale": "10k",
                    "result_cardinality": "20",
                    "selected_access_path": "collection_scan",
                    "access_path_evidence_source": "preflight",
                    "fallback_reason": "none",
                    "fallback_evidence_source": "preflight",
                    "storage_reads": "40",
                    "candidate_count": "20",
                    "peak_query_memory_bytes": "4096",
                    "worker_count": "1",
                    "configured_worker_count": "1",
                    "leaked_active_operator_workers": "0",
                    "worker_leak_evidence_source": "runtime_metrics",
                    "setup_time_ns": "1000",
                    "measurement_time_ns": "2000",
                    "execution_result_cache_hits": "0",
                    "failed_operations": "0"
                }}
            }}],
            {overrides}
        }}"#
    )
}

fn validate(value: &str) -> Result<(), String> {
    validate_complete_benchmark_contract(
        value,
        "expected-commit",
        "expected-toolchain",
        "default",
        &["perf.query.10k"],
    )
}

fn owner_manifest(
    tier: u32,
    scenarios: &[&str],
    result_cache_scenarios: &[&str],
) -> BenchmarkOwnerManifest {
    BenchmarkOwnerManifest {
        tier,
        scenarios: scenarios
            .iter()
            .map(|scenario| (*scenario).to_string())
            .collect(),
        result_cache_scenarios: result_cache_scenarios
            .iter()
            .map(|scenario| (*scenario).to_string())
            .collect(),
    }
}

fn suite_manifest() -> BenchmarkSuiteManifest {
    BTreeMap::from([
        (
            TIER1_OWNER.to_string(),
            owner_manifest(1, &[TIER1_SCENARIO], &[]),
        ),
        (
            TIER2_OWNER.to_string(),
            owner_manifest(
                2,
                &[TIER2_SCENARIO, RESULT_CACHE_SCENARIO],
                &[RESULT_CACHE_SCENARIO],
            ),
        ),
        (
            TIER5_OWNER.to_string(),
            owner_manifest(5, &[TIER5_SCENARIO], &[]),
        ),
    ])
}

fn suite_artifact(
    owner: &str,
    tier: u32,
    scenarios: &[(&str, u64)],
    total_elapsed_ns: u64,
) -> String {
    let summaries = scenarios
        .iter()
        .map(|(scenario_id, cache_hits)| {
            let scenario = benchmark_for_scenario(scenario_id).expect("registered scenario");
            assert_eq!(scenario.benchmark, owner);
            assert_eq!(scenario.declared_tier.number(), tier);
            serde_json::json!({
                "tier": tier,
                "intent": expected_intent(scenario.timing_mode),
                "metadata": {
                    "scenario_id": scenario_id,
                    "benchmark": owner,
                    "workload": scenario.workload,
                    "fixture_scale": scenario.fixture_scale,
                    "fixture_rows": scenario.fixture_rows.to_string(),
                    "fixture_class": format!("{:?}", scenario.fixture_class).to_ascii_lowercase(),
                    "operation_unit": scenario.operation_unit,
                    "result_cardinality": "20",
                    "selected_access_path": scenario
                        .expected_selected_access_path()
                        .unwrap_or(scenario.access_family),
                    "access_path_evidence_source": "preflight",
                    "fallback_reason": "none",
                    "fallback_evidence_source": "preflight",
                    "storage_reads": "40",
                    "candidate_count": "20",
                    "peak_query_memory_bytes": "4096",
                    "worker_count": scenario.worker_count.unwrap_or(0).to_string(),
                    "configured_worker_count": scenario.worker_count.unwrap_or(0).to_string(),
                    "leaked_active_operator_workers": "0",
                    "worker_leak_evidence_source": "runtime_metrics",
                    "setup_time_ns": "1000",
                    "measurement_time_ns": "2000",
                    "execution_result_cache_hits": cache_hits.to_string(),
                    "failed_operations": "0",
                    "signal_role": scenario.evidence_role.signal_role()
                }
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "schema_version": "cntryl-stress.v2",
        "suite": owner,
        "run_profile": "default",
        "metadata": {
            "filtered_run": "false",
            "owner_suite_complete": "true",
            "run_id": "complete-suite-run"
        },
        "environment": {
            "git_commit": "expected-commit",
            "rustc_version": "expected-toolchain"
        },
        "summaries": summaries,
        "total_elapsed_ns": total_elapsed_ns
    })
    .to_string()
}

fn expected_intent(timing_mode: BenchmarkTimingMode) -> &'static str {
    match timing_mode {
        BenchmarkTimingMode::Micro | BenchmarkTimingMode::Measure => "general",
        BenchmarkTimingMode::Counted | BenchmarkTimingMode::External => "external",
        BenchmarkTimingMode::Batch => "batch",
    }
}

fn mutate_scenario_summary(
    artifacts: &mut BTreeMap<String, String>,
    owner: &str,
    scenario_id: &str,
    mutate: impl FnOnce(&mut serde_json::Value),
) {
    let artifact = artifacts.get_mut(owner).expect("owner artifact");
    let mut value: serde_json::Value = serde_json::from_str(artifact).expect("artifact JSON");
    let summary = value["summaries"]
        .as_array_mut()
        .expect("summaries")
        .iter_mut()
        .find(|summary| summary["metadata"]["scenario_id"] == scenario_id)
        .expect("scenario summary");
    mutate(summary);
    *artifact = value.to_string();
}

fn suite_artifacts() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            TIER1_OWNER.to_string(),
            suite_artifact(TIER1_OWNER, 1, &[(TIER1_SCENARIO, 0)], 100 * SECOND),
        ),
        (
            TIER2_OWNER.to_string(),
            suite_artifact(
                TIER2_OWNER,
                2,
                &[(TIER2_SCENARIO, 0), (RESULT_CACHE_SCENARIO, 7)],
                200 * SECOND,
            ),
        ),
        (
            TIER5_OWNER.to_string(),
            suite_artifact(TIER5_OWNER, 5, &[(TIER5_SCENARIO, 0)], 3_600 * SECOND),
        ),
    ])
}

fn tier6_suite_artifacts(
    configured_seconds: u64,
    per_sample_seconds: u64,
    measured_samples: u64,
    measured_wall_seconds: u64,
) -> (BenchmarkSuiteManifest, BTreeMap<String, String>) {
    let manifest = BTreeMap::from([(
        TIER6_OWNER.to_string(),
        owner_manifest(6, &[TIER6_SCENARIO], &[]),
    )]);
    let mut artifact: serde_json::Value = serde_json::from_str(&suite_artifact(
        TIER6_OWNER,
        6,
        &[(TIER6_SCENARIO, 0)],
        measured_wall_seconds.saturating_mul(SECOND),
    ))
    .expect("Tier 6 artifact JSON");
    artifact["metadata"]["soak_total_duration_seconds"] =
        serde_json::json!(configured_seconds.to_string());
    artifact["metadata"]["soak_per_sample_duration_seconds"] =
        serde_json::json!(per_sample_seconds.to_string());
    artifact["metadata"]["soak_measured_samples"] = serde_json::json!(measured_samples.to_string());
    artifact["summaries"][0]["total_wall_clock_ns"] =
        serde_json::json!(measured_wall_seconds.saturating_mul(SECOND));
    (
        manifest,
        BTreeMap::from([(TIER6_OWNER.to_string(), artifact.to_string())]),
    )
}

fn replace_artifact_value(
    artifacts: &mut BTreeMap<String, String>,
    owner: &str,
    from: &str,
    to: &str,
) {
    let artifact = artifacts.get_mut(owner).expect("owner artifact");
    *artifact = artifact.replacen(from, to, 1);
}

#[test]
fn should_reject_warmed_query_results() {
    // Arrange
    let value = artifact(r#""unused": true"#).replace(
        r#""execution_result_cache_hits": "0""#,
        r#""execution_result_cache_hits": "1""#,
    );

    // Act
    let error = validate(&value).expect_err("warmed result must fail");

    // Assert
    assert!(error.contains("execution result cache"));
}

#[test]
fn should_reject_filtered_owner_runs() {
    // Arrange
    let value = artifact(r#""unused": true"#)
        .replace(r#""filtered_run": "false""#, r#""filtered_run": "true""#);

    // Act
    let error = validate(&value).expect_err("filtered run must fail");

    // Assert
    assert!(error.contains("filtered"));
}

#[test]
fn should_reject_stale_benchmark_commits() {
    // Arrange
    let value = artifact(r#""unused": true"#).replace("expected-commit", "stale-commit");

    // Act
    let error = validate(&value).expect_err("stale commit must fail");

    // Assert
    assert!(error.contains("git commit"));
}

#[test]
fn should_reject_incomplete_owner_suites() {
    // Arrange
    let value = artifact(r#""unused": true"#);

    // Act
    let error = validate_complete_benchmark_contract(
        &value,
        "expected-commit",
        "expected-toolchain",
        "default",
        &["perf.query.10k", "perf.query.100k"],
    )
    .expect_err("incomplete suite must fail");

    // Assert
    assert!(error.contains("missing scenarios"));
}

#[test]
fn should_reject_missing_access_path_evidence() {
    // Arrange
    let value = artifact(r#""unused": true"#).replace(
        r#""selected_access_path": "collection_scan""#,
        r#""selected_access_path": """#,
    );

    // Act
    let error = validate(&value).expect_err("missing access path must fail");

    // Assert
    assert!(error.contains("selected_access_path"));
}

#[test]
fn should_reject_registry_declaration_as_access_path_evidence() {
    // Arrange
    let manifest = suite_manifest();
    let mut artifacts = suite_artifacts();
    mutate_scenario_summary(&mut artifacts, TIER5_OWNER, TIER5_SCENARIO, |summary| {
        summary["metadata"]["access_path_evidence_source"] = serde_json::json!("registry");
    });

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("registry-declared access path must not satisfy observed evidence");

    // Assert
    assert!(error.contains("observed selected_access_path"));
}

#[test]
fn should_reject_mislabeled_vector_access_path_artifact() {
    // Arrange
    let manifest = BTreeMap::from([(
        TIER3_VECTOR_OWNER.to_string(),
        owner_manifest(3, &[TIER3_VECTOR_SCENARIO], &[]),
    )]);
    let mut artifacts = BTreeMap::from([(
        TIER3_VECTOR_OWNER.to_string(),
        suite_artifact(
            TIER3_VECTOR_OWNER,
            3,
            &[(TIER3_VECTOR_SCENARIO, 0)],
            100 * SECOND,
        ),
    )]);
    mutate_scenario_summary(
        &mut artifacts,
        TIER3_VECTOR_OWNER,
        TIER3_VECTOR_SCENARIO,
        |summary| {
            summary["metadata"]["selected_access_path"] = serde_json::json!("collection_scan");
        },
    );

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("mislabeled vector access path must fail");

    // Assert
    assert!(error.contains("selected access path mismatch"));
}

#[test]
fn should_require_preflight_source_for_vector_access_path_artifact() {
    // Arrange
    let manifest = BTreeMap::from([(
        TIER3_VECTOR_OWNER.to_string(),
        owner_manifest(3, &[TIER3_VECTOR_SCENARIO], &[]),
    )]);
    let mut artifacts = BTreeMap::from([(
        TIER3_VECTOR_OWNER.to_string(),
        suite_artifact(
            TIER3_VECTOR_OWNER,
            3,
            &[(TIER3_VECTOR_SCENARIO, 0)],
            100 * SECOND,
        ),
    )]);
    mutate_scenario_summary(
        &mut artifacts,
        TIER3_VECTOR_OWNER,
        TIER3_VECTOR_SCENARIO,
        |summary| {
            summary["metadata"]["access_path_evidence_source"] = serde_json::json!("operation");
        },
    );

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("vector access path without preflight source must fail");

    // Assert
    assert!(error.contains("preflight selected access path evidence"));
}

#[test]
fn should_reject_leaked_active_operator_workers() {
    // Arrange
    let manifest = suite_manifest();
    let mut artifacts = suite_artifacts();
    mutate_scenario_summary(&mut artifacts, TIER5_OWNER, TIER5_SCENARIO, |summary| {
        summary["metadata"]["leaked_active_operator_workers"] = serde_json::json!(1);
    });

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("leaked workers must fail the resource gate");

    // Assert
    assert!(error.contains("leaked active operator workers"));
}

#[test]
fn should_reject_unobserved_worker_leak_evidence() {
    // Arrange
    let manifest = suite_manifest();
    let mut artifacts = suite_artifacts();
    mutate_scenario_summary(&mut artifacts, TIER5_OWNER, TIER5_SCENARIO, |summary| {
        summary["metadata"]["worker_leak_evidence_source"] = serde_json::json!("configuration");
    });

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("configured zero must not masquerade as observed worker cleanup");

    // Assert
    assert!(error.contains("observed worker-leak evidence"));
}

#[test]
fn should_reject_configured_worker_count_given_registry_mismatch() {
    // Arrange
    let manifest = suite_manifest();
    let mut artifacts = suite_artifacts();
    mutate_scenario_summary(&mut artifacts, TIER5_OWNER, TIER5_SCENARIO, |summary| {
        summary["metadata"]["configured_worker_count"] = serde_json::json!(4);
    });

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("configured workers must agree with the registered scaling row");

    // Assert
    assert!(error.contains("configured worker count mismatch"));
}

#[test]
fn should_reject_placeholder_instead_of_observed_numeric_evidence() {
    // Arrange
    let value = artifact(r#""unused": true"#).replace(
        r#""storage_reads": "40""#,
        r#""storage_reads": "storage.data.reads""#,
    );

    // Act
    let error = validate(&value).expect_err("placeholder evidence must fail");

    // Assert
    assert!(error.contains("numeric metadata.storage_reads"));
}

#[test]
fn should_route_filtered_artifacts_to_diagnostics() {
    // Arrange
    let root = Path::new("target/stress");

    // Act
    let output = artifact_output_dir(root, true);

    // Assert
    assert_eq!(output, PathBuf::from("target/stress/diagnostic"));
}

#[test]
fn should_accept_one_complete_benchmark_artifact_manifest() {
    // Arrange
    let manifest = suite_manifest();
    let artifacts = suite_artifacts();

    // Act
    let result = validate_complete_benchmark_artifacts(&manifest, &artifacts);

    // Assert
    assert_eq!(result, Ok(()));
}

#[test]
fn should_accept_complete_tier6_artifact_given_one_hour_endurance_evidence() {
    // Arrange
    let (manifest, artifacts) = tier6_suite_artifacts(3_600, 720, 5, 3_600);

    // Act
    let result = validate_complete_benchmark_artifacts(&manifest, &artifacts);

    // Assert
    assert_eq!(result, Ok(()));
}

#[test]
fn should_reject_complete_tier6_artifact_given_shortened_duration() {
    // Arrange
    let (manifest, artifacts) = tier6_suite_artifacts(5, 5, 1, 5);

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("shortened Tier 6 evidence must not satisfy the complete manifest");

    // Assert
    assert!(error.contains("at least 3600"));
}

#[test]
fn should_reject_complete_tier6_artifact_given_short_measured_wall_time() {
    // Arrange
    let (manifest, artifacts) = tier6_suite_artifacts(3_600, 720, 5, 3_599);

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("configured duration must be backed by measured wall time");

    // Assert
    assert!(error.contains("measured only"));
}

#[test]
fn should_reject_complete_summary_given_registry_owner_or_workload_mismatch() {
    // Arrange
    let mutations = [
        ("benchmark", "tier1_hotpath_keys"),
        ("workload", "key_encode_decode"),
    ];

    for (field, replacement) in mutations {
        let manifest = suite_manifest();
        let mut artifacts = suite_artifacts();
        mutate_scenario_summary(&mut artifacts, TIER1_OWNER, TIER1_SCENARIO, |summary| {
            summary["metadata"][field] = serde_json::json!(replacement);
        });

        // Act
        let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
            .expect_err("registry identity mismatch must fail");

        // Assert
        assert!(
            error.contains(field),
            "unexpected error for {field}: {error}"
        );
    }
}

#[test]
fn should_reject_complete_summary_given_registry_fixture_mismatch() {
    // Arrange
    let mutations = [
        ("fixture_scale", serde_json::json!("100k")),
        ("fixture_rows", serde_json::json!(100_000)),
        ("fixture_class", serde_json::json!("representative")),
    ];

    for (field, replacement) in mutations {
        let manifest = suite_manifest();
        let mut artifacts = suite_artifacts();
        mutate_scenario_summary(&mut artifacts, TIER1_OWNER, TIER1_SCENARIO, |summary| {
            summary["metadata"][field] = replacement;
        });

        // Act
        let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
            .expect_err("registry fixture mismatch must fail");

        // Assert
        assert!(
            error.contains(field),
            "unexpected error for {field}: {error}"
        );
    }
}

#[test]
fn should_reject_complete_summary_given_registry_evidence_contract_mismatch() {
    // Arrange
    let mutations = [("operation_unit", "key"), ("signal_role", "informational")];

    for (field, replacement) in mutations {
        let manifest = suite_manifest();
        let mut artifacts = suite_artifacts();
        mutate_scenario_summary(&mut artifacts, TIER1_OWNER, TIER1_SCENARIO, |summary| {
            summary["metadata"][field] = serde_json::json!(replacement);
        });

        // Act
        let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
            .expect_err("registry evidence mismatch must fail");

        // Assert
        assert!(
            error.contains(field),
            "unexpected error for {field}: {error}"
        );
    }
}

#[test]
fn should_reject_complete_summary_given_registry_timing_mismatch() {
    // Arrange
    let manifest = suite_manifest();
    let mut artifacts = suite_artifacts();
    mutate_scenario_summary(&mut artifacts, TIER5_OWNER, TIER5_SCENARIO, |summary| {
        summary["intent"] = serde_json::json!("external");
    });

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("registry timing mismatch must fail");

    // Assert
    assert!(error.contains("timing mode"));
}

#[test]
fn should_reject_measured_result_cache_scenario_given_zero_hits() {
    // Arrange
    let manifest = suite_manifest();
    let mut artifacts = suite_artifacts();
    mutate_scenario_summary(
        &mut artifacts,
        TIER2_OWNER,
        RESULT_CACHE_SCENARIO,
        |summary| {
            summary["metadata"]["execution_result_cache_hits"] = serde_json::json!("0");
        },
    );

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("measured result-cache scenario must observe hits");

    // Assert
    assert!(error.contains("measured result-cache"));
}

#[test]
fn should_reject_manifest_cache_policy_given_registry_mismatch() {
    // Arrange
    let mut manifest = suite_manifest();
    manifest
        .get_mut(TIER2_OWNER)
        .expect("Tier 2 owner")
        .result_cache_scenarios
        .clear();
    let artifacts = suite_artifacts();

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("manifest cache policy must match the registry");

    // Assert
    assert!(error.contains("result-cache policy"));
}

#[test]
fn should_derive_complete_manifest_from_registered_owners() {
    // Arrange
    let scenarios = benchmark_scenarios().collect::<Vec<_>>();
    let expected_owners = scenarios
        .iter()
        .map(|scenario| scenario.benchmark)
        .collect::<BTreeSet<_>>();

    // Act
    let manifest = expected_complete_benchmark_manifest().expect("registered manifest");

    // Assert
    assert_eq!(
        manifest.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        expected_owners
    );
    for scenario in scenarios {
        let owner = manifest.get(scenario.benchmark).expect("scenario owner");
        assert_eq!(owner.tier, scenario.declared_tier.number());
        assert!(owner.scenarios.contains(scenario.scenario_id));
        assert_eq!(
            owner.result_cache_scenarios.contains(scenario.scenario_id),
            scenario.result_cache_policy == ResultCachePolicy::Measured
        );
    }
}

#[test]
fn should_reject_mixed_complete_suite_run_ids() {
    // Arrange
    let manifest = suite_manifest();
    let mut artifacts = suite_artifacts();
    replace_artifact_value(
        &mut artifacts,
        TIER2_OWNER,
        "complete-suite-run",
        "different-run",
    );

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("mixed run IDs must fail");

    // Assert
    assert!(error.contains("run ID"));
}

#[test]
fn should_reject_empty_complete_suite_run_id() {
    // Arrange
    let manifest = suite_manifest();
    let mut artifacts = suite_artifacts();
    for artifact in artifacts.values_mut() {
        *artifact = artifact.replacen("complete-suite-run", "", 1);
    }

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("empty run ID must fail");

    // Assert
    assert!(error.contains("nonempty CASSIE run ID"));
}

#[test]
fn should_reject_mixed_complete_suite_commits() {
    // Arrange
    let manifest = suite_manifest();
    let mut artifacts = suite_artifacts();
    replace_artifact_value(
        &mut artifacts,
        TIER2_OWNER,
        "expected-commit",
        "different-commit",
    );

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("mixed commits must fail");

    // Assert
    assert!(error.contains("git commit"));
}

#[test]
fn should_reject_mixed_complete_suite_toolchains() {
    // Arrange
    let manifest = suite_manifest();
    let mut artifacts = suite_artifacts();
    replace_artifact_value(
        &mut artifacts,
        TIER2_OWNER,
        "expected-toolchain",
        "different-toolchain",
    );

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("mixed toolchains must fail");

    // Assert
    assert!(error.contains("toolchain"));
}

#[test]
fn should_reject_mixed_complete_suite_profiles() {
    // Arrange
    let manifest = suite_manifest();
    let mut artifacts = suite_artifacts();
    replace_artifact_value(
        &mut artifacts,
        TIER2_OWNER,
        r#""run_profile":"default""#,
        r#""run_profile":"other""#,
    );

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("mixed profiles must fail");

    // Assert
    assert!(error.contains("profile"));
}

#[test]
fn should_reject_missing_complete_suite_owner() {
    // Arrange
    let manifest = suite_manifest();
    let mut artifacts = suite_artifacts();
    artifacts.remove(TIER2_OWNER);

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("missing owner must fail");

    // Assert
    assert!(error.contains("missing owners"));
}

#[test]
fn should_reject_filtered_complete_suite_owner() {
    // Arrange
    let manifest = suite_manifest();
    let mut artifacts = suite_artifacts();
    replace_artifact_value(
        &mut artifacts,
        TIER2_OWNER,
        r#""filtered_run":"false""#,
        r#""filtered_run":"true""#,
    );

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("filtered owner must fail");

    // Assert
    assert!(error.contains("filtered"));
}

#[test]
fn should_reject_incomplete_complete_suite_owner() {
    // Arrange
    let manifest = suite_manifest();
    let mut artifacts = suite_artifacts();
    replace_artifact_value(
        &mut artifacts,
        TIER2_OWNER,
        r#""owner_suite_complete":"true""#,
        r#""owner_suite_complete":"false""#,
    );

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("incomplete owner must fail");

    // Assert
    assert!(error.contains("owner_suite_complete"));
}

#[test]
fn should_reject_missing_complete_suite_scenario() {
    // Arrange
    let manifest = suite_manifest();
    let mut artifacts = suite_artifacts();
    let artifact = artifacts.get_mut(TIER2_OWNER).expect("Tier 2 artifact");
    let mut value: serde_json::Value = serde_json::from_str(artifact).expect("artifact JSON");
    value["summaries"].as_array_mut().expect("summaries").pop();
    *artifact = value.to_string();

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("missing scenario must fail");

    // Assert
    assert!(error.contains("missing scenarios"));
}

#[test]
fn should_reject_unexpected_complete_suite_scenario() {
    // Arrange
    let manifest = suite_manifest();
    let mut artifacts = suite_artifacts();
    let artifact = artifacts.get_mut(TIER1_OWNER).expect("Tier 1 artifact");
    let mut value: serde_json::Value = serde_json::from_str(artifact).expect("artifact JSON");
    let summaries = value["summaries"].as_array_mut().expect("summaries");
    let mut unexpected = summaries.first().expect("summary").clone();
    unexpected["metadata"]["scenario_id"] = serde_json::json!("perf.unexpected.scenario");
    summaries.push(unexpected);
    *artifact = value.to_string();

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("unexpected scenario must fail");

    // Assert
    assert!(error.contains("unexpected scenarios"));
}

#[test]
fn should_reject_execution_result_cache_contamination() {
    // Arrange
    let manifest = suite_manifest();
    let mut artifacts = suite_artifacts();
    let artifact = artifacts.get_mut(TIER1_OWNER).expect("Tier 1 artifact");
    *artifact = artifact.replacen(
        r#""execution_result_cache_hits":"0""#,
        r#""execution_result_cache_hits":"1""#,
        1,
    );

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("cache contamination must fail");

    // Assert
    assert!(error.contains("execution result cache"));
}

#[test]
fn should_reject_tier_one_through_four_wall_time_over_nine_hundred_seconds() {
    // Arrange
    let manifest = suite_manifest();
    let mut artifacts = suite_artifacts();
    let artifact = artifacts.get_mut(TIER2_OWNER).expect("Tier 2 artifact");
    let mut value: serde_json::Value = serde_json::from_str(artifact).expect("artifact JSON");
    value["total_elapsed_ns"] = serde_json::json!(801 * SECOND);
    *artifact = value.to_string();

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("Tier 1-4 time over 900 seconds must fail");

    // Assert
    assert!(error.contains("900 seconds"));
}

#[test]
fn should_reject_smoke_profile_as_complete_suite_evidence() {
    // Arrange
    let manifest = suite_manifest();
    let mut artifacts = suite_artifacts();
    for artifact in artifacts.values_mut() {
        *artifact = artifact.replacen(r#""run_profile":"default""#, r#""run_profile":"smoke""#, 1);
    }

    // Act
    let error = validate_complete_benchmark_artifacts(&manifest, &artifacts)
        .expect_err("smoke evidence must fail");

    // Assert
    assert!(error.contains("smoke"));
}

#[test]
fn should_reject_diagnostic_path_as_complete_suite_evidence() {
    // Arrange
    let root = Path::new("target/stress/diagnostic");

    // Act
    let error = validate_complete_benchmark_suite(root)
        .expect_err("diagnostic artifacts must not satisfy the suite");

    // Assert
    assert!(error.contains("diagnostic"));
}

#[test]
#[ignore = "requires a complete unfiltered cargo bench --bench '*' run with CASSIE_BENCH_RUN_ID"]
fn should_validate_complete_benchmark_artifact_manifest() {
    // Arrange
    let root = Path::new("target/stress");

    // Act
    let result = validate_complete_benchmark_suite(root);

    // Assert
    assert_eq!(result, Ok(()));
}
