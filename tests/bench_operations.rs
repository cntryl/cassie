// Consolidated integration suite: bench_operations.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "../benches/support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "../benches/support/stress.rs"]
#[allow(dead_code)]
mod stress;
#[path = "support/data_dir.rs"]
mod support_data_dir;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "../benches/support/workloads.rs"]
mod workloads;

// Formerly tests/benchmark_evidence_contract.rs.
mod benchmark_evidence_contract {
    use super::performance_benchmarks;

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

    #[test]
    fn should_accept_summary_wall_time_as_measurement_evidence() {
        // Arrange
        let mut value: serde_json::Value =
            serde_json::from_str(&artifact("\"marker\": true")).expect("benchmark artifact");
        value["summaries"][0]["metadata"]
            .as_object_mut()
            .expect("summary metadata")
            .remove("measurement_time_ns");
        value["summaries"][0]["total_wall_clock_ns"] = serde_json::json!(2_000_u64);

        // Act
        let validation = validate(&value.to_string());

        // Assert
        validation.expect("summary total wall time should be measurement evidence");
    }

    #[test]
    fn should_reject_malformed_legacy_timing_even_with_summary_wall_time() {
        // Arrange
        let mut value: serde_json::Value =
            serde_json::from_str(&artifact("\"marker\": true")).expect("benchmark artifact");
        value["summaries"][0]["metadata"]["measurement_time_ns"] =
            serde_json::json!("not-a-duration");
        value["summaries"][0]["total_wall_clock_ns"] = serde_json::json!(2_000_u64);

        // Act
        let error = validate(&value.to_string()).expect_err("malformed legacy timing must fail");

        // Assert
        assert!(error.contains("numeric metadata.measurement_time_ns"));
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
        artifact["metadata"]["soak_measured_samples"] =
            serde_json::json!(measured_samples.to_string());
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
            *artifact =
                artifact.replacen(r#""run_profile":"default""#, r#""run_profile":"smoke""#, 1);
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
}

// Formerly tests/benchmark_harness_contract.rs.
mod benchmark_harness_contract {
    use super::performance_benchmarks;
    use super::stress;
    use super::workloads;

    use std::collections::BTreeSet;
    use std::time::Duration;
    use std::{cell::Cell, panic::AssertUnwindSafe};

    use cassie::types::Value;
    use serde_json::json;

    #[test]
    fn should_register_paired_fsst_codec_kernels() {
        // Arrange
        let scenarios = performance_benchmarks::benchmark_scenarios()
            .map(|scenario| scenario.scenario_id)
            .collect::<BTreeSet<_>>();

        // Act
        let has_encode = scenarios.contains("perf.kernel.fsst_codec_encode");
        let has_decode = scenarios.contains("perf.kernel.fsst_codec_decode");

        // Assert
        assert!(has_encode);
        assert!(has_decode);
    }

    #[test]
    fn should_register_paired_alp_codec_kernels() {
        // Arrange
        let scenarios = performance_benchmarks::benchmark_scenarios()
            .map(|scenario| scenario.scenario_id)
            .collect::<BTreeSet<_>>();

        // Act
        let has_encode = scenarios.contains("perf.kernel.alp_codec_encode");
        let has_decode = scenarios.contains("perf.kernel.alp_codec_decode");

        // Assert
        assert!(has_encode);
        assert!(has_decode);
    }

    #[test]
    fn should_keep_per_sample_measurement_time_out_of_invariant_metadata() {
        // Arrange
        let harness = include_str!("../benches/support/stress.rs");

        // Act
        let volatile_metadata_writes = harness
            .matches("ctx.metadata(\"measurement_time_ns\"")
            .count();

        // Assert
        assert_eq!(
            volatile_metadata_writes, 0,
            "cntryl-stress 0.4 requires measurement metadata to be invariant across samples"
        );
    }

    #[test]
    fn should_reverse_each_benchmark_pair_on_alternating_invocations() {
        // Arrange
        let owner = include_str!("../benches/tier2_subsystem_column_scan.rs");

        // Act
        let first = stress::stress_interleaved::alternating_pair_execution_order(8, 0);
        let second = stress::stress_interleaved::alternating_pair_execution_order(8, 1);
        let filtered = stress::stress_interleaved::alternating_pair_execution_order(3, 1);

        // Assert
        assert_eq!(first, vec![0, 1, 1, 0, 2, 3, 3, 2, 4, 5, 5, 4, 6, 7, 7, 6]);
        assert_eq!(second, vec![1, 0, 0, 1, 3, 2, 2, 3, 5, 4, 4, 5, 7, 6, 6, 7]);
        assert_eq!(filtered, vec![1, 0, 0, 1, 2, 2]);
        assert!(owner.contains("measure_counted_interleaved"));
    }

    #[test]
    fn should_sample_each_benchmark_pair_in_an_independent_group() {
        // Arrange
        let case_count = 8;

        // Act
        let groups = stress::stress_interleaved::adjacent_pair_execution_groups(case_count);
        let filtered_groups = stress::stress_interleaved::adjacent_pair_execution_groups(3);

        // Assert
        assert_eq!(groups, vec![0..2, 2..4, 4..6, 6..8]);
        assert_eq!(filtered_groups, vec![0..2, 2..3]);
    }

    #[test]
    fn should_balance_pair_chunks_within_each_sampling_invocation() {
        // Arrange
        let iterations_per_case = 8;

        // Act
        let first = stress::stress_interleaved::alternating_pair_chunk_execution_order(
            2,
            iterations_per_case,
            0,
        );
        let second = stress::stress_interleaved::alternating_pair_chunk_execution_order(
            2,
            iterations_per_case,
            1,
        );
        let filtered = stress::stress_interleaved::alternating_pair_chunk_execution_order(1, 4, 0);

        // Assert
        assert_eq!(first, vec![0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 1]);
        assert_eq!(second, vec![1, 0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 1, 0]);
        assert_eq!(filtered, vec![0, 0, 0, 0]);
        assert_eq!(
            first.iter().filter(|case| **case == 0).count(),
            iterations_per_case
        );
        assert_eq!(
            first.iter().filter(|case| **case == 1).count(),
            iterations_per_case
        );
    }

    fn tier1_row_case(
        fixture_class: performance_benchmarks::FixtureClass,
        fixture_rows: usize,
        operation_unit: stress::OperationUnit,
    ) -> stress::StressCase {
        stress::StressCase::new("row_encode_decode", "micro").runtime_contract(
            stress::FixtureDeclaration::new(
                fixture_class,
                fixture_rows,
                "tier1_hotpath_row_codec/micro",
            ),
            operation_unit,
        )
    }

    #[test]
    fn should_allow_incomplete_selector_case_before_setup() {
        // Arrange
        let runner = stress::CassieStressRunner::new(
            "tier1_hotpath_row_codec",
            performance_benchmarks::BenchmarkTier::Tier1,
        );
        let selector = stress::StressCase::new("row_encode_decode", "micro");

        // Act
        let enabled = runner.is_enabled(&selector);

        // Assert
        assert!(enabled);
    }

    #[test]
    fn should_type_every_registered_operation_unit() {
        // Arrange
        let registered = performance_benchmarks::benchmark_scenarios()
            .map(|scenario| scenario.operation_unit)
            .collect::<BTreeSet<_>>();

        // Act
        let typed = stress::OperationUnit::ALL
            .into_iter()
            .map(stress::OperationUnit::as_str)
            .collect::<BTreeSet<_>>();

        // Assert
        assert_eq!(typed, registered);
    }

    #[test]
    fn should_construct_each_owner_with_explicit_benchmark_tier() {
        // Arrange
        let owners = performance_benchmarks::benchmark_scenarios()
            .map(|scenario| (scenario.benchmark, scenario.declared_tier))
            .collect::<BTreeSet<_>>();
        let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

        // Act
        let implicit_owners = owners
            .into_iter()
            .filter_map(|(owner, tier)| {
                let source =
                    std::fs::read_to_string(repository.join("benches").join(format!("{owner}.rs")))
                        .expect("benchmark owner source");
                let source = source.split_whitespace().collect::<String>();
                let constructor = format!(
                    "stress::runner(performance_benchmarks::BenchmarkTier::Tier{},",
                    tier.number()
                );
                (!source.contains(&constructor)).then_some(owner)
            })
            .collect::<Vec<_>>();
        let harness = include_str!("../benches/support/stress.rs");

        // Assert
        assert!(
            implicit_owners.is_empty(),
            "implicit owners: {implicit_owners:?}"
        );
        assert!(!harness.contains("benchmark_tier_from_owner"));
    }

    #[test]
    fn should_reject_missing_runtime_declaration_before_measurement_closure() {
        // Arrange
        let invoked = Cell::new(false);
        let mut runner = stress::CassieStressRunner::new(
            "tier1_hotpath_row_codec",
            performance_benchmarks::BenchmarkTier::Tier1,
        );
        let case = stress::StressCase::new("row_encode_decode", "micro");

        // Act
        let panic = std::panic::catch_unwind(AssertUnwindSafe(|| {
            runner.measure_micro(case, || {
                invoked.set(true);
                1_usize
            });
        }));

        // Assert
        assert!(panic.is_err());
        assert!(!invoked.get());
    }

    #[test]
    fn should_reject_runtime_fixture_class_mismatch_before_measurement_closure() {
        // Arrange
        let invoked = Cell::new(false);
        let mut runner = stress::CassieStressRunner::new(
            "tier1_hotpath_row_codec",
            performance_benchmarks::BenchmarkTier::Tier1,
        );
        let case = tier1_row_case(
            performance_benchmarks::FixtureClass::Subsystem,
            0,
            stress::OperationUnit::Row,
        );

        // Act
        let panic = std::panic::catch_unwind(AssertUnwindSafe(|| {
            runner.measure_micro(case, || {
                invoked.set(true);
                1_usize
            });
        }));

        // Assert
        assert!(panic.is_err());
        assert!(!invoked.get());
    }

    #[test]
    fn should_reject_runtime_fixture_size_mismatch_before_measurement_closure() {
        // Arrange
        let invoked = Cell::new(false);
        let mut runner = stress::CassieStressRunner::new(
            "tier1_hotpath_row_codec",
            performance_benchmarks::BenchmarkTier::Tier1,
        );
        let case = tier1_row_case(
            performance_benchmarks::FixtureClass::Kernel,
            1,
            stress::OperationUnit::Row,
        );

        // Act
        let panic = std::panic::catch_unwind(AssertUnwindSafe(|| {
            runner.measure_micro(case, || {
                invoked.set(true);
                1_usize
            });
        }));

        // Assert
        assert!(panic.is_err());
        assert!(!invoked.get());
    }

    #[test]
    fn should_reject_runtime_operation_unit_mismatch_before_measurement_closure() {
        // Arrange
        let invoked = Cell::new(false);
        let mut runner = stress::CassieStressRunner::new(
            "tier1_hotpath_row_codec",
            performance_benchmarks::BenchmarkTier::Tier1,
        );
        let case = tier1_row_case(
            performance_benchmarks::FixtureClass::Kernel,
            0,
            stress::OperationUnit::Key,
        );

        // Act
        let panic = std::panic::catch_unwind(AssertUnwindSafe(|| {
            runner.measure_micro(case, || {
                invoked.set(true);
                1_usize
            });
        }));

        // Assert
        assert!(panic.is_err());
        assert!(!invoked.get());
    }

    #[test]
    fn should_allow_same_owner_scale_fixture_identity_reuse() {
        // Arrange
        let mut tracker = stress::FixtureIdentityTracker::default();

        // Act
        let first = tracker.register("tier5_scaling_query", "100k", "query-disk-100k");
        let reopened = tracker.register("tier5_scaling_query", "100k", "query-disk-100k");

        // Assert
        assert_eq!(first, Ok(()));
        assert_eq!(reopened, Ok(()));
    }

    #[test]
    fn should_reject_different_fixture_identity_for_same_owner_scale() {
        // Arrange
        let mut tracker = stress::FixtureIdentityTracker::default();
        tracker
            .register("tier5_scaling_query", "100k", "query-disk-100k")
            .expect("first fixture identity");

        // Act
        let error = tracker
            .register("tier5_scaling_query", "100k", "second-query-disk-100k")
            .expect_err("different owner-scale fixture identity must fail");

        // Assert
        assert!(error.contains("different fixture identities"));
    }

    #[test]
    fn should_record_external_batch_elapsed_once() {
        // Arrange
        let elapsed = Duration::from_millis(250);
        let completed_operations = 500;

        // Act
        let recorded = stress::external_elapsed(elapsed, completed_operations);

        // Assert
        assert_eq!(recorded, elapsed);
    }

    #[test]
    fn should_normalize_external_runtime_counters_per_completed_operation() {
        // Arrange
        let first_candidate_count = 1_080;
        let first_completed_operations = 54;
        let second_candidate_count = 1_020;
        let second_completed_operations = 51;

        // Act
        let first = stress::normalize_runtime_counter(
            first_candidate_count,
            Some(first_completed_operations),
        );
        let second = stress::normalize_runtime_counter(
            second_candidate_count,
            Some(second_completed_operations),
        );
        let fixed_operation = stress::normalize_runtime_counter(first_candidate_count, None);

        // Assert
        assert_eq!(first, 20);
        assert_eq!(second, 20);
        assert_eq!(fixed_operation, first_candidate_count);
    }

    #[test]
    fn should_default_soak_duration_to_one_hour() {
        // Arrange
        let environment = None;
        let command_line = None;

        // Act
        let duration = stress::resolve_soak_duration(environment, command_line)
            .expect("default soak duration");

        // Assert
        assert_eq!(duration.total, Duration::from_hours(1));
        assert_eq!(duration.source, "default");
    }

    #[test]
    fn should_prefer_cli_soak_duration_over_environment() {
        // Arrange
        let environment = Some("120");
        let command_line = Some("30");

        // Act
        let duration =
            stress::resolve_soak_duration(environment, command_line).expect("CLI soak duration");

        // Assert
        assert_eq!(duration.total, Duration::from_secs(30));
        assert_eq!(duration.source, "cli");
    }

    #[test]
    fn should_divide_tier6_duration_across_measured_samples() {
        // Arrange
        let total = Duration::from_hours(1);
        let measured_samples = 5;

        // Act
        let per_sample = stress::soak_sample_duration(total, measured_samples)
            .expect("per-sample soak duration");

        // Assert
        assert_eq!(per_sample, Duration::from_mins(12));
    }

    #[test]
    fn should_reject_zero_soak_duration() {
        // Arrange
        let zero_duration = Some("0");

        // Act
        let duration_error = stress::resolve_soak_duration(zero_duration, None)
            .expect_err("zero soak duration must fail");

        // Assert
        assert!(duration_error.contains("positive"));
    }

    #[test]
    fn should_reject_shortened_tier6_duration_outside_smoke_profile() {
        // Arrange
        let shortened = Duration::from_secs(5);

        // Act
        let error = stress::validate_soak_duration_for_profile(shortened, false)
            .expect_err("shortened canonical endurance evidence must fail");

        // Assert
        assert!(error.contains("only valid with STRESS_PROFILE=smoke"));
    }

    #[test]
    fn should_allow_shortened_tier6_duration_given_smoke_profile() {
        // Arrange
        let shortened = Duration::from_secs(5);

        // Act
        let result = stress::validate_soak_duration_for_profile(shortened, true);

        // Assert
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn should_reject_zero_measured_samples() {
        // Arrange
        let measured_samples = 0;

        // Act
        let sample_error = stress::soak_sample_duration(Duration::from_secs(1), measured_samples)
            .expect_err("zero measured samples must fail");

        // Assert
        assert!(sample_error.contains("samples"));
    }

    #[test]
    fn should_require_observed_preflight_for_scaling_queries() {
        // Arrange
        let scenario =
            performance_benchmarks::benchmark_for_scenario("perf.scale.query.relational.10k")
                .expect("registered Tier 5 query");

        // Act
        let error = stress::validate_preflight_requirement(scenario, None)
            .expect_err("unobserved scaling query evidence must fail");

        // Assert
        assert!(error.contains("observed preflight"));
    }

    #[test]
    fn should_map_vector_families_to_persisted_access_paths() {
        // Arrange
        let cases = [
            ("perf.vector.executor.100k", "vector_exact"),
            ("perf.vector.hnsw_persisted.100k", "hnsw"),
            ("perf.vector.ivfflat_persisted.100k", "ivfflat"),
        ];

        // Act
        let observed = cases.map(|(scenario_id, _)| {
            performance_benchmarks::benchmark_for_scenario(scenario_id)
                .expect("registered vector scenario")
                .expected_selected_access_path()
        });

        // Assert
        assert_eq!(
            observed,
            [Some("vector_exact"), Some("hnsw"), Some("ivfflat")]
        );
    }

    #[test]
    fn should_reject_mislabeled_vector_preflight_before_measurement() {
        // Arrange
        let scenario =
            performance_benchmarks::benchmark_for_scenario("perf.vector.hnsw_persisted.100k")
                .expect("registered HNSW scenario");
        let preflight = stress::PreflightEvidence::new("collection_scan", "none");

        // Act
        let error = stress::validate_preflight_requirement(scenario, Some(&preflight))
            .expect_err("mislabeled vector preflight must fail");

        // Assert
        assert!(error.contains("perf.vector.hnsw_persisted.100k"));
        assert!(error.contains("hnsw"));
        assert!(error.contains("collection_scan"));
    }

    #[test]
    fn should_scope_candidate_count_to_access_family() {
        // Arrange
        let delta = json!({
            "query": { "rows_returned_total": 91 },
            "search": {
                "candidate_count_total": 17
            },
            "vector": {
                "candidate_count_total": 73
            }
        });

        // Act
        let candidates = stress::scoped_candidate_count(&delta, "fulltext");

        // Assert
        assert_eq!(candidates, 17);
    }

    #[test]
    fn should_ignore_unrelated_family_fallback_metrics() {
        // Arrange
        let delta = json!({
            "search": { "row_scan_fallback_total": 0 },
            "vector": { "hnsw_fallbacks": 4, "row_scan_fallback_total": 2 }
        });
        let current = json!({
            "search": { "last_fallback_reason": "" },
            "vector": { "last_fallback_reason": "unrelated_vector_fallback" }
        });

        // Act
        let fallback = stress::scoped_fallback_evidence(&delta, &current, "fulltext");

        // Assert
        assert_eq!(fallback.count, 0);
        assert_eq!(fallback.reason, "none");
    }

    #[test]
    fn should_report_only_selected_family_fallback_reason() {
        // Arrange
        let delta = json!({
            "search": { "row_scan_fallback_total": 3 },
            "vector": { "hnsw_fallbacks": 9 }
        });
        let current = json!({
            "search": { "last_fallback_reason": "posting_generation_mismatch" },
            "vector": { "last_fallback_reason": "unrelated_vector_fallback" }
        });

        // Act
        let fallback = stress::scoped_fallback_evidence(&delta, &current, "fulltext");

        // Assert
        assert_eq!(fallback.count, 3);
        assert_eq!(fallback.reason, "posting_generation_mismatch");
    }

    #[test]
    fn should_use_access_family_candidate_metrics_before_result_rows() {
        // Arrange
        let delta = json!({
            "query": { "rows_returned_total": 999 },
            "joins": { "left_input_rows_total": 30, "right_input_rows_total": 40 },
            "parallel_aggregation": { "rows": 80 },
            "graph": { "rows": 90, "candidates": 95 },
            "aggregate_acceleration": { "accelerated_segments": 97 },
            "time_series": { "index_entries_scanned": 100, "rows": 50 },
            "read_paths": { "collection_scan_rows": 110, "ordered_rows": 120 },
            "search": { "candidate_count_total": 10 },
            "vector": { "candidate_count_total": 20 },
            "hybrid": { "candidate_count_total": 30 }
        });

        // Act
        let observed = [
            stress::scoped_candidate_count(&delta, "join"),
            stress::scoped_candidate_count(&delta, "worker_saturation"),
            stress::scoped_candidate_count(&delta, "graph"),
            stress::scoped_candidate_count(&delta, "column_analytics"),
            stress::scoped_candidate_count(&delta, "time_series"),
            stress::scoped_candidate_count(&delta, "relational_index"),
            stress::scoped_candidate_count(&delta, "mixed_load"),
        ];

        // Assert
        assert_eq!(observed, [70, 80, 95, 97, 100, 230, 290]);
    }

    #[test]
    fn should_use_time_series_rows_when_no_index_entries_are_observed() {
        // Arrange
        let delta = json!({
            "query": { "rows_returned_total": 999 },
            "time_series": { "index_entries_scanned": 0, "rows": 50 }
        });

        // Act
        let candidates = stress::scoped_candidate_count(&delta, "time_series");

        // Assert
        assert_eq!(candidates, 50);
    }

    #[test]
    fn should_scope_time_series_storage_reads_to_logical_buckets() {
        // Arrange
        let cold = json!({
            "storage": { "data": { "reads": 1 } },
            "time_series": { "buckets_scanned": 8 }
        });
        let cached = json!({
            "storage": { "data": { "reads": 0 } },
            "time_series": { "buckets_scanned": 8 }
        });

        // Act
        let cold = stress::scoped_storage_read_observation(&cold, "time_series_bucket_native");
        let cached = stress::scoped_storage_read_observation(&cached, "time_series_bucket_native");

        // Assert
        assert_eq!(cold, cached);
        assert_eq!(cold.count, 8);
        assert_eq!(cold.unit, "time_series_bucket");
    }

    #[test]
    fn should_scope_range_scan_storage_reads_to_logical_index_probes() {
        // Arrange
        let cold = json!({ "storage": { "data": { "reads": 1 } } });
        let cached = json!({ "storage": { "data": { "reads": 0 } } });

        // Act
        let cold = stress::scoped_storage_read_observation(&cold, "range_scan");
        let cached = stress::scoped_storage_read_observation(&cached, "range_scan");

        // Assert
        assert_eq!(cold, cached);
        assert_eq!(cold.count, 1);
        assert_eq!(cold.unit, "relational_index_probe");
    }

    #[test]
    fn should_preserve_runtime_storage_reads_for_other_access_paths() {
        // Arrange
        let delta = json!({
            "graph": { "reads": 3 },
            "storage": {
                "data": { "reads": 5 },
                "schema": { "reads": 2 }
            },
            "time_series": { "buckets_scanned": 99 }
        });

        // Act
        let observed = stress::scoped_storage_read_observation(&delta, "collection_scan");

        // Assert
        assert_eq!(observed.count, 10);
        assert_eq!(observed.unit, "runtime_storage_read");
    }

    #[test]
    fn should_accumulate_retrieval_setup_only_in_explicit_setup_sections() {
        // Arrange
        let source = include_str!("../benches/tier5_scaling_retrieval.rs");

        // Act
        let explicit_setup_sections = source.matches("accumulate_setup(").count();

        // Assert
        assert!(explicit_setup_sections > 1);
        assert!(!source.contains("setup_started.elapsed()"));
        assert!(source.contains("fn accumulate_setup<T>"));
        assert!(source.contains("prepare_fulltext_warm_state"));
        assert!(source.contains("evidenced(fulltext, *setup_time"));
        assert!(source.contains("evidenced(cases.ivf, *setup_time"));
        assert!(source.contains("ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES.to_string()"));
    }

    #[test]
    fn should_construct_one_shared_fixture_for_tier3_query_owner() {
        // Arrange
        let source = include_str!("../benches/tier3_system_query.rs");

        // Act
        let shared_fixture_constructions =
            source.matches("workloads::tier3_query_context(").count();
        let obsolete_fixture_constructors = [
            "workloads::context(",
            "workloads::vectorized_join_context(",
            "workloads::graph_context(",
            "workloads::time_series_context(",
        ]
        .into_iter()
        .filter(|constructor| source.contains(constructor))
        .collect::<Vec<_>>();

        // Assert
        assert_eq!(shared_fixture_constructions, 1);
        assert!(obsolete_fixture_constructors.is_empty());
        assert!(source.contains("prepare_tier3_query_domains"));
    }

    #[test]
    fn should_clean_up_shared_tier3_query_fixture() {
        // Arrange
        let source = include_str!("../benches/tier3_system_query.rs");

        // Act
        let shuts_down_cassie = source.contains("context.cassie.shutdown()");
        let drops_context = source.contains("drop(context)");
        let finishes_runner = source.contains("runner.finish()");
        let removes_data_dir = source.contains("std::fs::remove_dir_all(&data_dir)");
        let removes_marker_file = source.contains("std::fs::remove_file(&data_dir)");
        let verifies_cleanup = source.contains("assert!(!data_dir.exists()");
        let finish_position = source.rfind("runner.finish()").expect("runner finish");
        let cleanup_position = source
            .find("std::fs::remove_dir_all(&data_dir)")
            .expect("fixture cleanup");

        // Assert
        assert!(shuts_down_cassie);
        assert!(drops_context);
        assert!(finishes_runner);
        assert!(removes_data_dir);
        assert!(removes_marker_file);
        assert!(verifies_cleanup);
        assert!(finish_position < cleanup_position);
    }

    #[test]
    fn should_require_bounded_result_evidence_from_tier3_specialized_queries() {
        // Arrange
        let source = include_str!("../benches/tier3_system_query.rs");

        // Act
        let evidence_helpers = [
            "execute_column_evidence",
            "execute_vector_evidence",
            "execute_graph_evidence",
            "assert_query_cleanup",
            "assert_metric_delta_bounded",
        ];
        let missing = evidence_helpers
            .into_iter()
            .filter(|helper| !source.contains(helper))
            .collect::<Vec<_>>();

        // Assert
        assert!(missing.is_empty(), "missing Tier 3 evidence: {missing:?}");
        assert!(source.contains("EXPECTED_COLUMN_ROW"));
        assert!(source.contains("EXPECTED_GRAPH_NODES"));
        assert!(source.contains("ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES"));
    }

    #[test]
    fn should_batch_tier3_column_queries_with_explicit_normalization() {
        // Arrange
        let source = include_str!("../benches/tier3_system_query.rs");

        // Act
        let has_batch_size = source.contains("const COLUMN_QUERIES_PER_BATCH: u64 = 8;");
        let has_query_unit = source.contains(".parameter(\"logical_unit\", \"query\")");
        let has_normalization =
            source.contains(".parameter(\"queries_per_logical_operation\", \"1\")");
        let measures_complete_batch =
            source.contains("runner.measure_batch(case, COLUMN_QUERIES_PER_BATCH, ||");
        let executes_complete_batch = source.contains("for _ in 0..COLUMN_QUERIES_PER_BATCH");

        // Assert
        assert!(has_batch_size);
        assert!(has_query_unit);
        assert!(has_normalization);
        assert!(measures_complete_batch);
        assert!(executes_complete_batch);
    }

    #[test]
    fn should_declare_tier3_mixed_workload_measurement_shape() {
        // Arrange
        let owner = include_str!("../benches/tier3_system_mixed_load.rs");
        let workload = include_str!("../benches/support/workloads/tier3.rs");

        // Act
        let has_logical_unit = owner.contains(".parameter(\"logical_unit\", \"operation\")");
        let has_normalization =
            owner.contains(".parameter(\"operations_per_logical_operation\", \"1\")");
        let has_fixed_workload =
            owner.contains(".metadata(\"measurement_shape\", \"fixed_workload\")");
        let uses_duration_batch = owner.contains("runner.measure_batch(case, 1, ||");
        let preserves_workflow = [
            "Tier 3 mixed relational query",
            "Tier 3 mixed ingest",
            "Tier 3 mixed point retrieval",
            "Tier 3 mixed full-text retrieval",
            "Tier 3 mixed cleanup count",
        ]
        .into_iter()
        .all(|step| workload.contains(step));

        // Assert
        assert!(has_logical_unit);
        assert!(has_normalization);
        assert!(has_fixed_workload);
        assert!(uses_duration_batch);
        assert!(preserves_workflow);
    }

    #[test]
    fn should_require_ordered_bounded_cleanup_evidence_from_tier4_portals() {
        // Arrange
        let source = include_str!("../benches/support/workloads/pgwire.rs");
        let owner = include_str!("../benches/tier4_integration_pgwire.rs");

        // Act
        let evidence_helpers = [
            "assert_ordered_disjoint_portal_pages",
            "assert_pgwire_read_bound",
            "assert_pgwire_query_cleanup",
        ];
        let missing = evidence_helpers
            .into_iter()
            .filter(|helper| !source.contains(helper))
            .collect::<Vec<_>>();

        // Assert
        assert!(missing.is_empty(), "missing Tier 4 evidence: {missing:?}");
        assert!(source.contains("Some(\"57014\")"));
        assert!(source.contains("cancelled portal returned no row page"));
        assert!(source.contains("let per_operation_candidate_bound"));
        assert!(source.contains("per_operation_candidate_bound.saturating_mul(query_operations)"));
        assert!(owner.contains("assert_eq!(fetches, 2"));
        assert!(owner.contains("assert_eq!(cancellations, 1"));
    }

    #[test]
    fn should_gate_scaling_projection_work_on_production_metric_names() {
        // Arrange
        let source = include_str!("../benches/support/workloads/scaling_legacy.rs");

        // Act
        let records_refresh = source.contains("materialized_refreshes");
        let records_verification = source.contains("integrity_verifications");

        // Assert
        assert!(records_refresh);
        assert!(records_verification);
    }

    #[test]
    fn should_normalize_scaling_lifecycle_commands_by_source_rows() {
        // Arrange
        let workflow_scenarios = [
            "perf.rebuild.refresh.10k",
            "perf.rebuild.refresh.100k",
            "perf.scale.rebuild.refresh.250k",
            "perf.time_series.retention.100k",
            "perf.time_series.rollup_refresh.100k",
            "perf.verification.full.100k",
        ];

        // Act
        let operation_units = workflow_scenarios.map(|scenario_id| {
            performance_benchmarks::benchmark_for_scenario(scenario_id)
                .expect("registered Tier 5 lifecycle scenario")
                .operation_unit
        });
        let replay_unit =
            performance_benchmarks::benchmark_for_scenario("perf.replay.lag_catchup.100k")
                .expect("registered Tier 5 replay scenario")
                .operation_unit;

        // Assert
        assert_eq!(operation_units, ["source_row"; 6]);
        assert_eq!(replay_unit, "event");
        let owner_source = include_str!("../benches/tier5_scaling_lifecycle.rs");
        assert!(owner_source.contains("let source_rows = u64::try_from(rows)"));
        assert_eq!(owner_source.matches("source_rows,").count(), 6);
    }

    #[test]
    fn should_prepare_time_series_mutations_outside_lifecycle_measurement() {
        // Arrange
        let setup_source = include_str!("../benches/support/workloads/scaling_legacy.rs");
        let timed_source = include_str!("../benches/support/workloads/system.rs");

        // Act
        let retention_body = timed_source
            .split_once("pub fn time_series_retention_enforcement")
            .expect("retention workload")
            .1
            .split_once("pub fn time_series_rollup_refresh")
            .expect("end of retention workload")
            .0;
        let rollup_body = timed_source
            .split_once("pub fn time_series_rollup_refresh")
            .expect("rollup workload")
            .1
            .split_once("pub fn timed_ingest_document")
            .expect("end of rollup workload")
            .0;

        // Assert
        assert!(setup_source.contains("ts-retention-expired-sentinel"));
        assert!(!retention_body.contains("put_documents"));
        assert!(!rollup_body.contains("put_documents"));
        assert!(retention_body.contains("\"enforcements\""));
        assert!(retention_body.contains("\"errors\""));
        assert!(rollup_body.contains("\"refreshes\""));
    }

    #[test]
    fn should_prepare_projection_replay_inputs_before_measurement() {
        // Arrange
        let owner_source = include_str!("../benches/tier5_scaling_lifecycle.rs");
        let workload_source = include_str!("../benches/support/workloads/scaling.rs");

        // Act
        let setup_position = owner_source
            .find("prepare_isolated_projection_replay_batches")
            .expect("replay input setup");
        let measurement_position = owner_source
            .find("runner.measure_batch(")
            .expect("replay measurement");
        let timed_replay_body = workload_source
            .split_once("pub fn isolated_projection_replay(")
            .expect("timed replay function")
            .1
            .split_once("pub fn drop_vector_index")
            .expect("end of timed replay function")
            .0;

        // Assert
        assert!(setup_position < measurement_position);
        assert!(owner_source.contains(".take_next()"));
        assert!(owner_source.contains("PROJECTION_REPLAY_EVENTS_PER_BATCH"));
        assert!(!timed_replay_body.contains("ProjectionReplayEvent"));
        assert!(!timed_replay_body.contains("scale-replay-event"));
    }

    #[test]
    fn should_construct_one_shared_fixture_given_tier2_ingest_cases() {
        // Arrange
        let owner_source = include_str!("../benches/tier2_subsystem_ingest.rs");

        // Act
        let constructor_count = owner_source.matches("ProjectionBatchFixture::new(").count();
        let setup_position = owner_source
            .find("ProjectionBatchFixture::new(")
            .expect("shared projection fixture setup");
        let filter_position = owner_source
            .find("write_enabled || replay_enabled")
            .expect("lazy shared fixture guard");
        let evidence_uses = owner_source
            .matches("runtime_evidence(fixture.cassie())")
            .count();
        let identity_uses = owner_source.matches("fixture.fixture_identity()").count();

        // Assert
        assert_eq!(constructor_count, 1);
        assert!(filter_position < setup_position);
        assert_eq!(evidence_uses, 2);
        assert_eq!(identity_uses, 1);
        assert!(!owner_source.contains("ProjectionBatchFixture::new_write"));
        assert!(!owner_source.contains("ProjectionBatchFixture::new_replay"));
    }

    #[test]
    fn should_normalize_preserved_analytical_queries_by_result_rows() {
        // Arrange
        let scenario_ids = [
            "perf.core_read.recursive_cte.100k",
            "perf.core_read.window_frames.100k",
        ];
        let owner_source = include_str!("../benches/tier5_scaling_query.rs");

        // Act
        let operation_units = scenario_ids.map(|scenario_id| {
            performance_benchmarks::benchmark_for_scenario(scenario_id)
                .expect("registered preserved analytical scenario")
                .operation_unit
        });

        // Assert
        assert_eq!(operation_units, ["result_row"; 2]);
        assert!(owner_source.contains("recursive_cte_result_rows(UPPER_BOUND)"));
        assert!(owner_source.contains("u64::try_from(expected_rows)"));
        assert!(owner_source.contains("u64::try_from(EXPECTED_ROWS)"));
    }

    #[test]
    fn should_label_dense_join_algorithm_selection_profile() {
        // Arrange
        let owner_source = include_str!("../benches/tier5_scaling_query.rs");
        let contract = include_str!("../docs/performance-contracts.md");

        // Act
        let artifact_profile_is_declared = owner_source.contains(
            "case.metadata(\"benchmark_resource_profile\", \"dense_stream_selection_4k\")",
        );
        let contract_names_profile =
            contract.contains("`benchmark_resource_profile=dense_stream_selection_4k`");

        // Assert
        assert!(artifact_profile_is_declared);
        assert!(contract_names_profile);
        assert!(contract.contains("4 KiB algorithm-selection profile"));
    }

    // Merged from tests/benchmark_soak_contract.rs to cut a separate test binary.
    #[test]
    fn should_cleanup_transport_soak_fixture_after_measurement() {
        // Arrange
        let source = include_str!("../benches/tier6_soak_transport.rs");

        // Act
        let shuts_down_cassie = source.contains("context.cassie.shutdown()");
        let removes_data_dir = source.contains("std::fs::remove_dir_all(&data_dir)");
        let removes_marker_file = source.contains("std::fs::remove_file(&data_dir)");
        let verifies_cleanup = source.contains("assert!(!data_dir.exists()");

        // Assert
        assert!(shuts_down_cassie);
        assert!(removes_data_dir);
        assert!(removes_marker_file);
        assert!(verifies_cleanup);
    }

    #[test]
    fn should_own_only_generated_tls_material_given_http_benchmark_configuration() {
        // Arrange
        let tls_source = include_str!("../benches/support/workloads/http.rs");
        let callers = [
            include_str!("../benches/tier4_integration_http.rs"),
            include_str!("../benches/tier4_integration_protocol_compare.rs"),
            include_str!("../benches/tier5_scaling_transport.rs"),
            include_str!("../benches/tier6_soak_transport.rs"),
        ];

        // Act
        let returns_generated_ownership =
            tls_source.contains("Result<Option<GeneratedHttpTlsMaterial>, CassieError>");
        let leaves_user_paths_unowned = tls_source.contains("return Ok(None);");
        let generated_material_has_cleanup = tls_source.contains("impl GeneratedHttpTlsMaterial")
            && tls_source.contains("pub fn cleanup(");
        let every_caller_retains_and_cleans = callers
            .iter()
            .all(|source| source.contains("generated_http_tls") && source.contains(".cleanup()"));

        // Assert
        assert!(returns_generated_ownership);
        assert!(leaves_user_paths_unowned);
        assert!(generated_material_has_cleanup);
        assert!(every_caller_retains_and_cleans);
    }

    #[test]
    fn should_scope_tier_four_http_query_to_fixture_database() {
        // Arrange
        let database = "benchmark_database";

        // Act
        let body = workloads::http_admin_query_body(database);

        // Assert
        assert_eq!(
            body,
            json!({
                "database": database,
                "sql": workloads::HTTP_ADMIN_QUERY,
            })
        );
    }

    #[test]
    fn should_enforce_configured_tier6_result_row_bounds() {
        // Arrange
        let mixed = include_str!("../benches/tier6_soak_mixed.rs");
        let transport = include_str!("../benches/tier6_soak_transport.rs");

        // Act
        let mixed_configures_bound = mixed.contains("TIER6_MAX_RESULT_ROWS")
            && mixed.contains("context_with_mock_tei_embeddings(")
            && mixed.contains("\"configured_max_result_rows\"");
        let transport_configures_bound = transport.contains("TIER6_MAX_RESULT_ROWS")
            && transport.contains("scalar_context(")
            && transport.contains("\"configured_max_result_rows\"");
        let every_mixed_result_is_gated = mixed.contains("assert_result_cardinality_within_bound(");
        let every_transport_result_is_gated = transport
            .matches("assert_result_cardinality_within_bound(")
            .count()
            >= 4;

        // Assert
        assert!(mixed_configures_bound);
        assert!(transport_configures_bound);
        assert!(every_mixed_result_is_gated);
        assert!(every_transport_result_is_gated);
    }

    // Merged from tests/benchmark_sql_contract.rs to cut a separate test binary.

    #[test]
    fn should_batch_declared_plan_cache_misses_per_timing_sample() {
        // Arrange
        let fixture = workloads::CacheFixture::new(1_024, false);
        let owner = include_str!("../benches/tier2_subsystem_plan_cache.rs");
        let scenario = performance_benchmarks::benchmark_for_scenario("perf.cache.plan_miss.1k")
            .expect("registered plan-cache miss scenario");
        let mut observed_lookups = 0_usize;

        // Act
        let completed =
            stress::repeat_counted_batch(workloads::PLAN_CACHE_MISS_LOOKUPS_PER_SAMPLE, || {
                observed_lookups = observed_lookups.saturating_add(1);
                u64::try_from(fixture.plan_miss()).expect("completed lookup count should fit u64")
            });

        // Assert
        assert_eq!(workloads::PLAN_CACHE_MISS_LOOKUPS_PER_SAMPLE, 1_024);
        assert_eq!(
            observed_lookups,
            workloads::PLAN_CACHE_MISS_LOOKUPS_PER_SAMPLE
        );
        assert_eq!(completed, 1_024);
        assert_eq!(
            scenario.timing_mode,
            performance_benchmarks::BenchmarkTimingMode::Counted
        );
        assert!(owner.contains("runner.measure_counted_batch("));
        assert!(owner.contains("workloads::PLAN_CACHE_MISS_LOOKUPS_PER_SAMPLE,"));
        assert!(owner.contains("fixture.plan_miss()"));
    }

    #[test]
    fn should_batch_json_serialization_with_exact_row_normalization() {
        // Arrange
        let fixture = workloads::ProtocolCodecFixture::new(512);
        let owner = include_str!("../benches/tier2_subsystem_protocol_handlers.rs");
        let scenario =
            performance_benchmarks::benchmark_for_scenario("perf.protocol.json_rows.512")
                .expect("registered JSON serialization scenario");
        let mut observed_invocations = 0_usize;

        // Act
        let completed =
            stress::repeat_counted_batch(workloads::PROTOCOL_JSON_INVOCATIONS_PER_SAMPLE, || {
                observed_invocations = observed_invocations.saturating_add(1);
                let completed_rows = fixture.json_serialization();
                assert_eq!(completed_rows, 512);
                completed_rows
            });

        // Assert
        assert_eq!(workloads::PROTOCOL_JSON_INVOCATIONS_PER_SAMPLE, 16);
        assert_eq!(
            observed_invocations,
            workloads::PROTOCOL_JSON_INVOCATIONS_PER_SAMPLE
        );
        assert_eq!(completed, 8_192);
        assert_eq!(
            scenario.timing_mode,
            performance_benchmarks::BenchmarkTimingMode::Counted
        );
        assert_eq!(scenario.operation_unit, "row");
        assert!(owner.contains("runner.measure_counted_batch("));
        assert!(owner.contains("workloads::PROTOCOL_JSON_INVOCATIONS_PER_SAMPLE,"));
        assert!(owner.contains("fixture.json_serialization()"));
    }

    #[test]
    fn should_batch_sql_parser_with_exact_statement_normalization() {
        // Arrange
        let fixture = workloads::ParserFixture::new(128);
        let owner = include_str!("../benches/tier2_subsystem_parser.rs");
        let scenario = performance_benchmarks::benchmark_for_scenario("perf.sql.parser.128")
            .expect("registered SQL parser scenario");
        let mut observed_invocations = 0_usize;

        // Act
        let completed =
            stress::repeat_counted_batch(workloads::PARSER_INVOCATIONS_PER_SAMPLE, || {
                observed_invocations = observed_invocations.saturating_add(1);
                let completed_statements = fixture.parse();
                assert_eq!(completed_statements, 128);
                completed_statements
            });

        // Assert
        assert_eq!(workloads::PARSER_INVOCATIONS_PER_SAMPLE, 64);
        assert_eq!(
            observed_invocations,
            workloads::PARSER_INVOCATIONS_PER_SAMPLE
        );
        assert_eq!(completed, 8_192);
        assert_eq!(
            scenario.timing_mode,
            performance_benchmarks::BenchmarkTimingMode::Counted
        );
        assert_eq!(scenario.operation_unit, "statement");
        assert!(owner.contains("runner.measure_counted_batch("));
        assert!(owner.contains("workloads::PARSER_INVOCATIONS_PER_SAMPLE,"));
        assert!(owner.contains("fixture.parse()"));
    }

    #[test]
    fn should_batch_pgwire_codec_with_exact_message_normalization() {
        // Arrange
        let fixture = workloads::ProtocolCodecFixture::new(512);
        let owner = include_str!("../benches/tier2_subsystem_protocol_handlers.rs");
        let scenario =
            performance_benchmarks::benchmark_for_scenario("perf.protocol.pgwire_codec.512")
                .expect("registered pgwire codec scenario");
        let mut observed_invocations = 0_usize;

        // Act
        let completed =
            stress::repeat_counted_batch(workloads::PROTOCOL_PGWIRE_INVOCATIONS_PER_SAMPLE, || {
                observed_invocations = observed_invocations.saturating_add(1);
                let completed_messages = fixture.pgwire_codec();
                assert_eq!(completed_messages, 512);
                completed_messages
            });

        // Assert
        assert_eq!(workloads::PROTOCOL_PGWIRE_INVOCATIONS_PER_SAMPLE, 256);
        assert_eq!(
            observed_invocations,
            workloads::PROTOCOL_PGWIRE_INVOCATIONS_PER_SAMPLE
        );
        assert_eq!(completed, 131_072);
        assert_eq!(
            scenario.timing_mode,
            performance_benchmarks::BenchmarkTimingMode::Counted
        );
        assert_eq!(scenario.operation_unit, "message");
        assert!(owner.contains("runner.measure_counted_batch("));
        assert!(owner.contains("workloads::PROTOCOL_PGWIRE_INVOCATIONS_PER_SAMPLE,"));
        assert!(owner.contains("fixture.pgwire_codec()"));
    }

    #[test]
    fn should_batch_prepared_statement_loop_with_exact_message_normalization() {
        // Arrange
        let fixture = workloads::ProtocolCodecFixture::new(512);
        let owner = include_str!("../benches/tier2_subsystem_protocol_handlers.rs");
        let scenario =
            performance_benchmarks::benchmark_for_scenario("perf.protocol.prepared_loop.512")
                .expect("registered prepared-statement scenario");
        let mut observed_invocations = 0_usize;

        // Act
        let completed = stress::repeat_counted_batch(
            workloads::PROTOCOL_PREPARED_INVOCATIONS_PER_SAMPLE,
            || {
                observed_invocations = observed_invocations.saturating_add(1);
                let completed_messages = fixture.prepared_loop();
                assert_eq!(completed_messages, 512);
                completed_messages
            },
        );

        // Assert
        assert_eq!(workloads::PROTOCOL_PREPARED_INVOCATIONS_PER_SAMPLE, 1_024);
        assert_eq!(
            observed_invocations,
            workloads::PROTOCOL_PREPARED_INVOCATIONS_PER_SAMPLE
        );
        assert_eq!(completed, 524_288);
        assert_eq!(
            scenario.timing_mode,
            performance_benchmarks::BenchmarkTimingMode::Counted
        );
        assert_eq!(scenario.operation_unit, "message");
        assert!(owner.contains("runner.measure_counted_batch("));
        assert!(owner.contains("workloads::PROTOCOL_PREPARED_INVOCATIONS_PER_SAMPLE,"));
        assert!(owner.contains("fixture.prepared_loop()"));
    }

    #[test]
    fn should_batch_cache_hits_with_exact_lookup_normalization() {
        // Arrange
        let fixture = workloads::CacheFixture::new(1_024, true);
        let owner = include_str!("../benches/tier2_subsystem_plan_cache.rs");
        let plan_scenario =
            performance_benchmarks::benchmark_for_scenario("perf.cache.plan_hit.1k")
                .expect("registered plan-cache hit scenario");
        let result_scenario =
            performance_benchmarks::benchmark_for_scenario("perf.cache.result_hit.1k")
                .expect("registered result-cache hit scenario");
        let mut plan_lookups = 0_usize;
        let mut result_lookups = 0_usize;

        // Act
        let completed_plan_lookups =
            stress::repeat_counted_batch(workloads::CACHE_HIT_LOOKUPS_PER_SAMPLE, || {
                plan_lookups = plan_lookups.saturating_add(1);
                let completed =
                    u64::try_from(fixture.plan_hit()).expect("plan-hit count should fit u64");
                assert_eq!(completed, 1);
                completed
            });
        let completed_result_lookups =
            stress::repeat_counted_batch(workloads::CACHE_HIT_LOOKUPS_PER_SAMPLE, || {
                result_lookups = result_lookups.saturating_add(1);
                let completed =
                    u64::try_from(fixture.result_hit()).expect("result-hit count should fit u64");
                assert_eq!(completed, 1);
                completed
            });

        // Assert
        assert_eq!(workloads::CACHE_HIT_LOOKUPS_PER_SAMPLE, 256);
        assert_eq!(plan_lookups, workloads::CACHE_HIT_LOOKUPS_PER_SAMPLE);
        assert_eq!(result_lookups, workloads::CACHE_HIT_LOOKUPS_PER_SAMPLE);
        assert_eq!(completed_plan_lookups, 256);
        assert_eq!(completed_result_lookups, 256);
        assert_eq!(
            plan_scenario.timing_mode,
            performance_benchmarks::BenchmarkTimingMode::Counted
        );
        assert_eq!(
            result_scenario.timing_mode,
            performance_benchmarks::BenchmarkTimingMode::Counted
        );
        assert_eq!(plan_scenario.operation_unit, "lookup");
        assert_eq!(result_scenario.operation_unit, "lookup");
        assert_eq!(owner.matches("runner.measure_counted_batch(").count(), 3);
        assert_eq!(
            owner
                .matches("workloads::CACHE_HIT_LOOKUPS_PER_SAMPLE,")
                .count(),
            2
        );
        assert!(owner.contains("fixture.plan_hit()"));
        assert!(owner.contains("fixture.result_hit()"));
    }

    #[test]
    fn should_batch_ivfflat_probes_with_exact_observation_evidence() {
        // Arrange
        let fixture = workloads::VectorCandidateFixture::new(1_024);
        let owner = include_str!("../benches/tier2_subsystem_vector.rs");
        let scenario =
            performance_benchmarks::benchmark_for_scenario("perf.vector.ivfflat_probes.1k")
                .expect("registered IVFFlat probe scenario");

        // Act
        let observation = fixture.ivfflat_batch(workloads::VECTOR_IVFFLAT_INVOCATIONS_PER_SAMPLE);

        // Assert
        assert_eq!(workloads::VECTOR_IVFFLAT_INVOCATIONS_PER_SAMPLE, 4_096);
        assert_eq!(observation.completed_operations(), 16_384);
        assert_eq!(observation.result_cardinality(), 16_384);
        assert_eq!(observation.candidate_count(), Some(16_384));
        assert_eq!(
            scenario.timing_mode,
            performance_benchmarks::BenchmarkTimingMode::Counted
        );
        assert_eq!(scenario.operation_unit, "probe");
        assert!(owner.contains("\"fixture_invocations_per_sample\""));
        assert!(owner.contains("workloads::VECTOR_IVFFLAT_INVOCATIONS_PER_SAMPLE"));
        assert!(owner.contains("fixture.ivfflat_batch("));
        observation.finish_sample();
    }

    #[test]
    fn should_batch_brute_force_candidates_with_exact_observation_evidence() {
        // Arrange
        let fixture = workloads::VectorCandidateFixture::new(1_024);
        let owner = include_str!("../benches/tier2_subsystem_vector.rs");
        let scenario =
            performance_benchmarks::benchmark_for_scenario("perf.vector.bruteforce_candidates.1k")
                .expect("registered brute-force candidate scenario");

        // Act
        let observation =
            fixture.brute_force_batch(workloads::VECTOR_BRUTE_FORCE_INVOCATIONS_PER_SAMPLE);

        // Assert
        assert_eq!(workloads::VECTOR_BRUTE_FORCE_INVOCATIONS_PER_SAMPLE, 512);
        assert_eq!(observation.completed_operations(), 524_288);
        assert_eq!(observation.result_cardinality(), 10_240);
        assert_eq!(observation.candidate_count(), Some(524_288));
        assert_eq!(
            scenario.timing_mode,
            performance_benchmarks::BenchmarkTimingMode::Counted
        );
        assert_eq!(scenario.operation_unit, "candidate");
        assert!(owner.contains("\"fixture_invocations_per_sample\""));
        assert!(owner.contains("workloads::VECTOR_BRUTE_FORCE_INVOCATIONS_PER_SAMPLE"));
        assert!(owner.contains("fixture.brute_force_batch("));
        observation.finish_sample();
    }

    #[test]
    fn should_batch_hnsw_candidates_with_exact_observation_evidence() {
        // Arrange
        let fixture = workloads::VectorCandidateFixture::new(1_024);
        let owner = include_str!("../benches/tier2_subsystem_vector.rs");
        let scenario =
            performance_benchmarks::benchmark_for_scenario("perf.vector.hnsw_candidates.1k")
                .expect("registered HNSW candidate scenario");
        let single = fixture.hnsw();
        let fixture_invocations = u64::try_from(workloads::VECTOR_HNSW_INVOCATIONS_PER_SAMPLE)
            .expect("batched HNSW invocation count should fit u64");
        let expected_completed = single
            .completed_operations()
            .checked_mul(fixture_invocations)
            .expect("batched HNSW completed count should fit u64");
        let expected_cardinality = single
            .result_cardinality()
            .checked_mul(fixture_invocations)
            .expect("batched HNSW result cardinality should fit u64");
        single.finish_sample();

        // Act
        let observation = fixture.hnsw_batch(workloads::VECTOR_HNSW_INVOCATIONS_PER_SAMPLE);

        // Assert
        assert_eq!(workloads::VECTOR_HNSW_INVOCATIONS_PER_SAMPLE, 256);
        assert_eq!(observation.completed_operations(), expected_completed);
        assert_eq!(observation.result_cardinality(), expected_cardinality);
        assert_eq!(observation.candidate_count(), Some(expected_completed));
        assert_eq!(
            scenario.timing_mode,
            performance_benchmarks::BenchmarkTimingMode::Counted
        );
        assert_eq!(scenario.operation_unit, "candidate");
        assert!(owner.contains("\"fixture_invocations_per_sample\""));
        assert!(owner.contains("workloads::VECTOR_HNSW_INVOCATIONS_PER_SAMPLE"));
        assert!(owner.contains("fixture.hnsw_batch("));
        observation.finish_sample();
    }

    #[test]
    fn should_batch_hybrid_fusion_with_exact_candidate_normalization() {
        // Arrange
        let fixture = workloads::HybridFusionFixture::new(2_048);
        let owner = include_str!("../benches/tier2_subsystem_hybrid.rs");
        let scenario = performance_benchmarks::benchmark_for_scenario("perf.hybrid.fusion.2k")
            .expect("registered hybrid-fusion scenario");

        // Act
        let observation = fixture.fuse_batch(workloads::HYBRID_FUSION_INVOCATIONS_PER_SAMPLE);

        // Assert
        assert_eq!(workloads::HYBRID_FUSION_INVOCATIONS_PER_SAMPLE, 256);
        assert_eq!(observation.completed_operations(), 524_288);
        assert_eq!(observation.result_cardinality(), 524_288);
        assert_eq!(observation.candidate_count(), Some(524_288));
        assert_eq!(
            scenario.timing_mode,
            performance_benchmarks::BenchmarkTimingMode::Counted
        );
        assert_eq!(scenario.operation_unit, "candidate");
        assert!(owner.contains("\"fixture_invocations_per_sample\""));
        assert!(owner.contains("workloads::HYBRID_FUSION_INVOCATIONS_PER_SAMPLE"));
        assert!(owner.contains("fixture.fuse_batch("));
        observation.finish_sample();
    }

    #[test]
    fn should_bind_dynamic_plan_cache_values_given_closed_alias_selection() {
        // Arrange
        let nonce = 17;

        // Act
        let statement = workloads::bound_plan_cache_miss(nonce);

        // Assert
        assert!(statement.sql.contains("id AS miss_17"));
        assert!(statement.sql.contains("score >= $1"));
        assert!(statement.sql.contains("status IN ($2, $3, $4)"));
        assert!(!statement.sql.contains("miss-17"));
        assert_eq!(statement.params[3], Value::String("miss-17".to_string()));
    }

    #[test]
    fn should_cycle_only_closed_plan_cache_identifiers_given_large_nonce() {
        // Arrange
        let first = workloads::bound_plan_cache_miss(0);

        // Act
        let cycled = workloads::bound_plan_cache_miss(64);

        // Assert
        assert_eq!(first.sql, cycled.sql);
        assert_ne!(first.params, cycled.params);
    }

    #[test]
    fn should_bind_recursive_values_given_dynamic_fixture() {
        // Arrange
        let upper_bound = 7;

        // Act
        let recursive = workloads::bound_recursive_cte(upper_bound);

        // Assert
        assert!(recursive.sql.contains("seq.n < $1"));
        assert!(!recursive.sql.contains("seq.n < 7"));
        assert_eq!(recursive.params, [Value::Int64(7)]);
    }

    #[test]
    fn should_bind_time_series_values_given_dynamic_fixture() {
        // Arrange
        let start = "2026-01-10T00:00:00Z";
        let end = "2026-01-12T00:00:00Z";

        // Act
        let time_series = workloads::bound_time_series_window(start, end);

        // Assert
        assert!(time_series.sql.contains("event_at >= $1"));
        assert!(time_series.sql.contains("event_at < $2"));
        assert!(!time_series.sql.contains("2026-01-10"));
        assert_eq!(time_series.params.len(), 2);
    }

    #[test]
    fn should_warm_the_same_bound_fulltext_statement_that_is_measured() {
        // Arrange
        let scaling = include_str!("../benches/support/workloads/scaling.rs");
        let legacy = include_str!("../benches/support/workloads/scaling_legacy.rs");
        let retrieval = include_str!("../benches/tier5_scaling_retrieval.rs");

        // Act
        let production_uses_shared_params = scaling.contains("fulltext_scaling_params()");
        let warmup_uses_shared_params =
            legacy.contains("super::scaling::fulltext_scaling_params()");
        let preflight_uses_shared_params =
            retrieval.contains("workloads::fulltext_scaling_params()");

        // Assert
        assert!(production_uses_shared_params);
        assert!(warmup_uses_shared_params);
        assert!(preflight_uses_shared_params);
    }
}
// Formerly tests/benchmark_kernels.rs.
mod benchmark_kernels {
    use super::workloads;

    use cassie::benchmark::{
        ExecutorKernel, PgwireParameterBindingKernel, RowCodecKernel, RowKeyKernel,
    };
    use cassie::types::Value;

    const TIER3_DOMAIN_TEST_ROWS: usize = workloads::BENCH_DOCUMENT_WRITE_BATCH_ROWS + 2;
    const TIER3_GRAPH_SQL: &str = "SELECT node_id FROM graph_expand($1, $2, $3, $4, $5, $6, $7)";
    const TIER3_TIME_SERIES_SQL: &str = "SELECT tenant, amount FROM bench_time_series_events WHERE event_at >= $1 AND event_at < $2 ORDER BY event_at LIMIT 512";

    #[test]
    fn should_prepare_registered_hotpath_fixtures_given_closed_workload_names() {
        // Arrange
        let registered_workloads = [
            "row_encode_decode",
            "key_encode_decode",
            "batch_filter",
            "batch_projection",
            "value_comparison",
            "tokenization",
            "row_to_pgwire_encoding",
            "predicate_evaluation",
            "top_k_heap_maintenance",
            "cosine_distance",
            "dot_product",
            "l2_distance",
            "bm25_scoring",
        ];

        // Act
        let results = registered_workloads.map(workloads::prepare_hotpath);

        // Assert
        assert!(results.into_iter().all(|result| result.is_ok()));
    }

    #[test]
    fn should_complete_every_logical_operation_in_fast_tier_one_batches() {
        // Arrange
        workloads::prepare_hotpath("tokenization").expect("prepare tokenization fixture");
        workloads::prepare_hotpath("bm25_scoring").expect("prepare BM25 fixture");
        workloads::prepare_hotpath("key_encode_decode").expect("prepare key fixture");
        workloads::prepare_hotpath("batch_projection").expect("prepare projection fixture");
        workloads::prepare_hotpath("value_comparison").expect("prepare comparison fixture");
        workloads::prepare_hotpath("row_to_pgwire_encoding").expect("prepare pgwire fixture");
        workloads::prepare_hotpath("predicate_evaluation").expect("prepare predicate fixture");
        workloads::prepare_hotpath("top_k_heap_maintenance").expect("prepare top-k fixture");
        workloads::prepare_hotpath("cosine_distance").expect("prepare cosine fixture");
        workloads::prepare_hotpath("dot_product").expect("prepare dot-product fixture");
        workloads::prepare_hotpath("l2_distance").expect("prepare L2 fixture");

        // Act
        let completed = [
            (
                workloads::row_encode_decode_batch(),
                workloads::ROW_CODEC_BATCH_SIZE,
            ),
            (
                workloads::key_encode_decode_batch(),
                workloads::KEY_CODEC_BATCH_SIZE,
            ),
            (
                workloads::batch_projection_batch(),
                workloads::PROJECTION_BATCH_SIZE,
            ),
            (
                workloads::value_comparison_batch(),
                workloads::SCALAR_EVALUATION_BATCH_SIZE,
            ),
            (
                workloads::row_to_pgwire_encoding_batch(),
                workloads::PGWIRE_ROW_BATCH_SIZE,
            ),
            (
                workloads::predicate_evaluation_batch(),
                workloads::SCALAR_EVALUATION_BATCH_SIZE,
            ),
            (
                workloads::top_k_update_batch().completed_operations(),
                workloads::TOP_K_BATCH_SIZE,
            ),
            (
                workloads::tokenization_batch(),
                workloads::TOKENIZATION_BATCH_SIZE,
            ),
            (workloads::bm25_score_batch(), workloads::BM25_BATCH_SIZE),
            (
                workloads::cosine_distance_batch(),
                workloads::VECTOR_DISTANCE_BATCH_SIZE,
            ),
            (
                workloads::dot_product_batch(),
                workloads::VECTOR_DISTANCE_BATCH_SIZE,
            ),
            (
                workloads::l2_distance_batch(),
                workloads::VECTOR_DISTANCE_BATCH_SIZE,
            ),
        ];

        // Assert
        assert_eq!(workloads::BM25_TERMS_PER_SCORE, 8);
        assert_eq!(workloads::VECTOR_DISTANCE_DIMENSIONS, 384);
        assert!(completed
            .into_iter()
            .all(|(actual, expected)| actual == expected));
    }

    #[test]
    fn should_reject_unregistered_hotpath_fixture_given_unknown_workload_name() {
        // Arrange
        let workload = "query_parameter_binding";

        // Act
        let result = workloads::prepare_hotpath(workload);

        // Assert
        assert_eq!(result, Err("unknown Tier 1 hot-path workload"));
    }

    #[test]
    fn should_round_trip_binary_row_given_production_codec_fixture_when_decoded() {
        // Arrange
        let kernel = RowCodecKernel::sample();

        // Act
        let encoded = kernel.encode();
        let decoded = kernel.decode(&encoded);

        // Assert
        assert!(encoded.starts_with(b"CRB2"));
        assert_eq!(&decoded, kernel.expected_row());
    }

    #[test]
    fn should_round_trip_row_identity_given_production_key_fixture_when_decoded() {
        // Arrange
        let kernel = RowKeyKernel::for_row(7, "doc-1");
        let other_relation = RowKeyKernel::for_row(8, "doc-1");

        // Act
        let (encoded, decoded) = kernel.encode_decode();
        let (encoded_again, decoded_again) = kernel.encode_decode();
        let (other_encoded, _) = other_relation.encode_decode();

        // Assert
        assert_eq!(decoded, "doc-1");
        assert_eq!(decoded_again, "doc-1");
        assert_eq!(encoded, encoded_again);
        assert_ne!(encoded, other_encoded);
        assert!(!encoded.windows(2).any(|window| window == b"v2"));
    }

    #[test]
    fn should_decode_typed_values_given_production_pgwire_bind_parameters() {
        // Arrange
        let kernel = PgwireParameterBindingKernel::with_parameters(4);

        // Act
        let decoded = kernel.decode();

        // Assert
        assert_eq!(
            decoded,
            vec![
                Value::Int64(42),
                Value::String("alpha".to_string()),
                Value::Bool(true),
                Value::Float64(3.5),
            ]
        );
    }

    #[test]
    fn should_match_predicate_given_executor_fixture_when_evaluated() {
        // Arrange
        let kernel = ExecutorKernel::sample();

        // Act
        let predicate_matches = kernel.predicate_matches();

        // Assert
        assert!(predicate_matches);
    }

    #[test]
    fn should_match_all_value_pairs_given_executor_fixture_when_compared() {
        // Arrange
        let kernel = ExecutorKernel::sample();

        // Act
        let comparison_matches = kernel.matching_value_comparisons();

        // Assert
        assert_eq!(comparison_matches, 8);
    }

    #[test]
    fn should_filter_rows_given_executor_fixture_when_batch_kernel_runs() {
        // Arrange
        let kernel = ExecutorKernel::sample();

        // Act
        let matched = kernel.filter_batch();

        // Assert
        assert_eq!(matched, 23);
    }

    #[test]
    fn should_project_selected_value_given_executor_fixture_when_projection_runs() {
        // Arrange
        let kernel = ExecutorKernel::sample();

        // Act
        let projected = kernel.project_row();

        // Assert
        assert_eq!(projected, vec![Value::String("alpha".to_string())]);
    }

    #[test]
    fn should_keep_highest_scores_given_executor_fixture_when_top_k_runs() {
        // Arrange
        let kernel = ExecutorKernel::sample();

        // Act
        let scores = kernel.top_k_scores();

        // Assert
        assert_eq!(scores, vec![9, 7, 4]);
    }

    #[test]
    fn should_report_candidates_separately_given_batched_top_k_kernel_observation() {
        // Arrange
        workloads::prepare_hotpath("top_k_heap_maintenance").expect("registered top-k workload");

        // Act
        let observation = workloads::top_k_update_batch();

        // Assert
        assert_eq!(
            observation.completed_operations(),
            workloads::TOP_K_BATCH_SIZE
        );
        assert_eq!(
            observation.result_cardinality(),
            3 * workloads::TOP_K_BATCH_SIZE
        );
        assert_eq!(
            observation.candidate_count(),
            Some(6 * workloads::TOP_K_BATCH_SIZE)
        );
    }

    #[test]
    fn should_count_observed_candidates_given_hnsw_subsystem_search() {
        // Arrange
        let fixture = workloads::VectorCandidateFixture::new(128);

        // Act
        let observation = fixture.hnsw();

        // Assert
        assert_eq!(
            observation.completed_operations(),
            observation
                .candidate_count()
                .expect("HNSW candidate evidence")
        );
        assert!(observation.result_cardinality() <= observation.completed_operations());
    }

    #[test]
    fn should_require_vector_execution_count_for_every_scaling_path() {
        // Arrange
        let index_kinds = [None, Some("hnsw"), Some("ivfflat")];

        // Act
        let required = index_kinds.map(workloads::vector_execution_count_is_required);

        // Assert
        assert_eq!(required, [true, true, true]);
    }

    #[test]
    fn should_bound_durable_fixture_document_write_batches() {
        // Arrange
        let dataset_rows = 100_001;

        // Act
        let batch_sizes = workloads::bench_document_write_batch_ranges(dataset_rows)
            .map(|range| range.len())
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(batch_sizes.iter().sum::<usize>(), dataset_rows);
        assert_eq!(batch_sizes.len(), 21);
        assert!(batch_sizes.iter().all(|size| *size <= 5_000));
    }

    #[test]
    fn should_build_fulltext_index_once_after_bounded_document_loading() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("benchmark fixture test runtime");
        cassie::midge::adapter::set_fulltext_maintenance_failure_point(true);

        // Act
        let context = runtime.block_on(workloads::context("fulltext-build-once", 16));
        cassie::midge::adapter::set_fulltext_maintenance_failure_point(false);
        let context = context.expect("full-index benchmark fixture");
        let fulltext_state = context
            .cassie
            .midge
            .get_persisted_fulltext_index_state(&context.collection, "bench_documents_body_idx")
            .expect("read benchmark full-text index")
            .expect("benchmark full-text index state");
        let fulltext_rows = context
            .cassie
            .execute_sql(
                &context.session,
                "SELECT id FROM bench_documents WHERE search(body, 'alpha') ORDER BY id",
                vec![],
            )
            .expect("query benchmark full-text index");
        let mut index_names = context
            .cassie
            .midge
            .list_indexes()
            .expect("list benchmark indexes")
            .into_iter()
            .filter(|index| {
                index.collection
                    == cassie::catalog::canonical_relation_name(
                        "postgres",
                        "public",
                        &context.collection,
                    )
            })
            .map(|index| index.name)
            .collect::<Vec<_>>();
        index_names.sort();

        // Assert
        assert!(!context
            .cassie
            .midge
            .has_fulltext_maintenance_debt(&context.collection, "bench_documents_body_idx",)
            .expect("read benchmark full-text maintenance debt"));
        assert_eq!(fulltext_state.total_documents, 16);
        assert_eq!(fulltext_state.documents_with_text, 16);
        assert_eq!(fulltext_rows.rows.len(), 6);
        assert_eq!(
            index_names,
            [
                "bench_documents_body_idx",
                "bench_documents_lower_title_idx",
                "bench_documents_score_idx",
                "bench_documents_status_score_idx",
                "bench_documents_title_idx",
            ]
            .map(|name| cassie::catalog::canonical_relation_name("postgres", "public", name))
        );
        workloads::assert_fixture_boundaries(&context, &context.collection, "doc-0", "doc-15");
        let data_dir = context.data_dir.clone();
        drop(context);
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn should_record_projection_lifecycle_metrics_given_small_scaling_fixture() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("benchmark lifecycle test runtime");
        let context = runtime
            .block_on(workloads::disk_context_with_temp_budget(
                "benchmark-lifecycle-metrics",
                16,
                workloads::ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES,
            ))
            .expect("small projection lifecycle fixture");
        workloads::prepare_projection_lifecycle(&context);
        let before = context.cassie.metrics();

        // Act
        let cardinality = runtime.block_on(workloads::projection_verify_existing(&context));
        let after = context.cassie.metrics();

        // Assert
        assert!(cardinality > 0);
        assert!(
            after["projections"]["integrity_verifications"]
                .as_u64()
                .unwrap_or_default()
                > before["projections"]["integrity_verifications"]
                    .as_u64()
                    .unwrap_or_default(),
            "projection metrics before={before} after={after}"
        );
        let data_dir = context.data_dir.clone();
        context.cassie.shutdown();
        drop(context);
        std::fs::remove_dir_all(data_dir).expect("clean up lifecycle metric fixture");
    }

    #[test]
    fn should_share_bounded_projection_fixture_given_write_and_replay_samples() {
        // Arrange
        let runtime = workloads::runtime();
        let fixture = workloads::ProjectionBatchFixture::new(&runtime, 8);
        let write_cassie = fixture.cassie();
        let replay_cassie = fixture.cassie();

        // Act
        let first_write = fixture.write_batch();
        let first_written = first_write.completed_operations();
        first_write.finish_sample();
        let first_replay = fixture.replay_batch();
        let first_replayed = first_replay.result_cardinality();
        first_replay.finish_sample();
        let second_write = fixture.write_batch();
        let second_written = second_write.completed_operations();
        second_write.finish_sample();
        let second_replay = fixture.replay_batch();
        let second_replayed = second_replay.result_cardinality();
        second_replay.finish_sample();
        let retained_documents = write_cassie
            .midge
            .scan_documents("bench_documents")
            .expect("shared projection fixture should remain readable");

        // Assert
        assert!(std::sync::Arc::ptr_eq(&write_cassie, &replay_cassie));
        assert_eq!(first_written, 8);
        assert_eq!(first_replayed, 8);
        assert_eq!(second_written, 8);
        assert_eq!(second_replayed, 8);
        assert_eq!(retained_documents.len(), 8);
        assert_eq!(fixture.retained_fixture_rows(), 8);
        assert!(fixture
            .fixture_identity()
            .contains("tier2-subsystem-projection"));
    }

    #[test]
    fn should_retain_at_most_2048_logical_rows_given_tier2_projection_fixture() {
        // Arrange
        let runtime = workloads::runtime();

        // Act
        let fixture = workloads::ProjectionBatchFixture::new(&runtime, 2_048);

        // Assert
        assert_eq!(fixture.retained_fixture_rows(), 2_048);
    }

    #[test]
    fn should_record_exact_vector_metrics_given_tier3_query_shape() {
        // Arrange
        const SQL: &str = "SELECT id, vector_distance(embedding, $1) AS distance FROM bench_documents ORDER BY distance ASC LIMIT 20";
        workloads::configure_tier3_environment();
        let runtime = workloads::runtime();
        let context = runtime
            .block_on(workloads::tier3_query_context(
                "tier3-exact-vector-metrics-test",
                32,
            ))
            .expect("shared Tier 3 fixture");
        let params = || {
            vec![Value::Vector(cassie::types::Vector::new(vec![
                1.0, 0.0, 0.0,
            ]))]
        };
        let before = context.cassie.metrics();

        // Act
        let rows = workloads::execute_expected_query(&context, SQL, params(), 20);
        let after = context.cassie.metrics();

        // Assert
        assert_eq!(rows, 20);
        assert_eq!(after["vector"]["count"].as_u64(), Some(1));
        assert_eq!(after["vector"]["candidate_count_total"].as_u64(), Some(32));
        assert_eq!(after["vector"]["result_count_total"].as_u64(), Some(20));
        assert_eq!(after["vector"]["hnsw_executions"].as_u64(), Some(0));
        assert_eq!(after["vector"]["ivfflat_executions"].as_u64(), Some(0));
        assert_eq!(before["vector"]["count"].as_u64(), Some(0));
        let data_dir = context.data_dir.clone();
        context.cassie.shutdown();
        drop(context);
        std::fs::remove_dir_all(data_dir).expect("clean up exact-vector metric fixture");
    }

    #[test]
    fn should_report_verified_vector_access_paths_given_persisted_state() {
        // Arrange
        const SQL: &str = "SELECT id, vector_distance(embedding, $1) AS distance FROM bench_documents ORDER BY distance ASC LIMIT 20";
        workloads::configure_tier3_environment();
        let runtime = workloads::runtime();
        let context = runtime
            .block_on(workloads::tier3_query_context(
                "tier3-vector-access-evidence-test",
                32,
            ))
            .expect("shared Tier 3 fixture");
        let params = || {
            vec![Value::Vector(cassie::types::Vector::new(vec![
                1.0, 0.0, 0.0,
            ]))]
        };

        // Act
        let exact = workloads::assert_vector_preflight(
            &context,
            SQL,
            params(),
            "collection=postgres.public.bench_documents",
            32,
            workloads::VectorAccessPath::Exact,
        );
        workloads::create_hnsw_index(&context);
        let hnsw = workloads::assert_vector_preflight(
            &context,
            SQL,
            params(),
            "collection=postgres.public.bench_documents",
            32,
            workloads::VectorAccessPath::Hnsw,
        );
        let incomplete_hnsw = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            workloads::assert_vector_preflight(
                &context,
                SQL,
                params(),
                "collection=postgres.public.bench_documents",
                31,
                workloads::VectorAccessPath::Hnsw,
            )
        }));
        let wrong_state = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            workloads::assert_vector_preflight(
                &context,
                SQL,
                params(),
                "collection=postgres.public.bench_documents",
                32,
                workloads::VectorAccessPath::IvfFlat,
            )
        }));
        workloads::drop_vector_index(&context);
        workloads::create_ivfflat_index(&context);
        let ivf = workloads::assert_vector_preflight(
            &context,
            SQL,
            params(),
            "collection=postgres.public.bench_documents",
            32,
            workloads::VectorAccessPath::IvfFlat,
        );

        // Assert
        assert_eq!(exact.selected_access_path, "vector_exact");
        assert_eq!(hnsw.selected_access_path, "hnsw");
        assert_eq!(ivf.selected_access_path, "ivfflat");
        assert!(incomplete_hnsw.is_err());
        assert!(wrong_state.is_err());
        let data_dir = context.data_dir.clone();
        context.cassie.shutdown();
        drop(context);
        std::fs::remove_dir_all(data_dir).expect("clean up vector access evidence fixture");
    }

    #[test]
    fn should_bound_hnsw_hybrid_candidates_given_default_maximum() {
        // Arrange
        const EXPECTED_CANDIDATE_BOUND: usize = 20 * 64;
        const FIXTURE_ROWS: usize = EXPECTED_CANDIDATE_BOUND + 1;
        const SQL: &str = "SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM bench_documents ORDER BY score DESC LIMIT 20";
        const _: () = assert!(FIXTURE_ROWS > EXPECTED_CANDIDATE_BOUND);
        let expected_candidate_bound =
            u64::try_from(EXPECTED_CANDIDATE_BOUND).expect("candidate bound fits u64");
        workloads::configure_tier3_environment();
        let runtime = workloads::runtime();
        let context = runtime
            .block_on(workloads::tier3_query_context(
                "tier3-hybrid-candidate-bound-test",
                FIXTURE_ROWS,
            ))
            .expect("hybrid candidate-bound fixture");
        workloads::create_hnsw_index(&context);
        let params = || {
            vec![
                Value::String("alpha".to_string()),
                Value::Vector(cassie::types::Vector::new(vec![1.0, 0.0, 0.0])),
            ]
        };
        let before = context.cassie.metrics();

        // Act
        let rows = workloads::execute_expected_query(&context, SQL, params(), 20);
        let after = context.cassie.metrics();

        // Assert
        let candidates = after["hybrid"]["candidate_count_total"]
            .as_u64()
            .unwrap_or_default()
            .saturating_sub(
                before["hybrid"]["candidate_count_total"]
                    .as_u64()
                    .unwrap_or_default(),
            );
        let candidate_row_fetches = after["hybrid"]["candidate_row_fetches_total"]
            .as_u64()
            .unwrap_or_default()
            .saturating_sub(
                before["hybrid"]["candidate_row_fetches_total"]
                    .as_u64()
                    .unwrap_or_default(),
            );
        assert_eq!(rows, 20);
        assert!(candidates > 0);
        assert!(candidate_row_fetches > 0);
        assert!(
            candidates <= expected_candidate_bound,
            "hybrid candidate count {candidates} exceeded {EXPECTED_CANDIDATE_BOUND}"
        );
        assert!(
            candidate_row_fetches <= expected_candidate_bound,
            "hybrid candidate row fetches {candidate_row_fetches} exceeded {EXPECTED_CANDIDATE_BOUND}"
        );
        assert_eq!(
            after["hybrid"]["prefilter_fallback_count_total"].as_u64(),
            before["hybrid"]["prefilter_fallback_count_total"].as_u64()
        );
        assert_eq!(
            after["hybrid"]["row_scan_fallback_total"].as_u64(),
            before["hybrid"]["row_scan_fallback_total"].as_u64()
        );
        let data_dir = context.data_dir.clone();
        context.cassie.shutdown();
        drop(context);
        std::fs::remove_dir_all(data_dir).expect("clean up hybrid candidate-bound fixture");
    }

    #[test]
    fn should_record_hybrid_fallback_given_missing_ann_state() {
        // Arrange
        const SQL: &str = "SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM bench_documents ORDER BY score DESC LIMIT 20";
        workloads::configure_tier3_environment();
        let runtime = workloads::runtime();
        let context = runtime
            .block_on(workloads::tier3_query_context(
                "tier3-hybrid-missing-ann-test",
                64,
            ))
            .expect("hybrid missing-ANN fixture");
        let params = vec![
            Value::String("alpha".to_string()),
            Value::Vector(cassie::types::Vector::new(vec![1.0, 0.0, 0.0])),
        ];
        let before = context.cassie.metrics();

        // Act
        let rows = workloads::execute_expected_query(&context, SQL, params, 20);
        let after = context.cassie.metrics();

        // Assert
        assert_eq!(rows, 20);
        assert!(
            after["hybrid"]["prefilter_fallback_count_total"]
                .as_u64()
                .unwrap_or_default()
                > before["hybrid"]["prefilter_fallback_count_total"]
                    .as_u64()
                    .unwrap_or_default()
        );
        assert!(
            after["hybrid"]["row_scan_fallback_total"]
                .as_u64()
                .unwrap_or_default()
                > before["hybrid"]["row_scan_fallback_total"]
                    .as_u64()
                    .unwrap_or_default()
        );
        assert_eq!(
            after["hybrid"]["retrieval_fallback_reasons"]["missing-ann-state"].as_u64(),
            Some(1)
        );
        let data_dir = context.data_dir.clone();
        context.cassie.shutdown();
        drop(context);
        std::fs::remove_dir_all(data_dir).expect("clean up hybrid missing-ANN fixture");
    }

    #[test]
    fn should_use_bucket_native_time_series_given_shared_tier3_fixture() {
        // Arrange
        const SQL: &str = "SELECT tenant, amount FROM bench_time_series_events WHERE event_at >= $1 AND event_at < $2 ORDER BY event_at LIMIT 512";
        workloads::configure_tier3_environment();
        let runtime = workloads::runtime();
        let context = runtime
            .block_on(workloads::tier3_query_context(
                "tier3-shared-time-series-test",
                16,
            ))
            .expect("shared Tier 3 fixture");
        workloads::prepare_tier3_query_domains(
            &context,
            16,
            workloads::Tier3QueryDomains {
                join: false,
                graph: false,
                time_series: true,
            },
        )
        .expect("shared Tier 3 time-series domain");
        let params = || {
            vec![
                Value::String("2026-01-09T00:00:00Z".to_string()),
                Value::String("2026-01-10T00:00:00Z".to_string()),
            ]
        };
        let preflight = workloads::assert_time_series_preflight(&context, SQL, params());
        let before = context.cassie.metrics();

        // Act
        let rows = workloads::execute_expected_query(&context, SQL, params(), 16);
        let after = context.cassie.metrics();

        // Assert
        assert_eq!(rows, 16);
        assert_eq!(preflight.selected_access_path, "time_series_bucket_native");
        assert_eq!(preflight.fallback_reason, "none");
        assert!(
            after["time_series"]["bucket_native_hits"]
                .as_u64()
                .unwrap_or_default()
                > before["time_series"]["bucket_native_hits"]
                    .as_u64()
                    .unwrap_or_default(),
            "time-series metrics before={before} after={after}"
        );
        assert_eq!(
            after["time_series"]["fallback_scans"].as_u64(),
            before["time_series"]["fallback_scans"].as_u64(),
            "time-series fixture must not fall back to row-backed reads"
        );
        assert!(
            context
                .cassie
                .catalog
                .get_rollup("bench_time_series_hourly")
                .is_none(),
            "Tier 3 window scans must not prepare the Tier 5 rollup fixture"
        );
        assert!(
            context
                .cassie
                .catalog
                .get_retention_policy("bench_time_series_retention")
                .is_none(),
            "Tier 3 window scans must not prepare the Tier 5 retention fixture"
        );
        let data_dir = context.data_dir.clone();
        context.cassie.shutdown();
        drop(context);
        std::fs::remove_dir_all(data_dir).expect("clean up shared Tier 3 test fixture");
    }

    #[test]
    fn should_batch_every_tier3_query_domain_without_losing_boundary_semantics() {
        // Arrange
        workloads::configure_tier3_environment();
        let runtime = workloads::runtime();
        let context = runtime
            .block_on(workloads::empty_tier3_query_context(
                "tier3-batched-query-domains-test",
                TIER3_DOMAIN_TEST_ROWS,
            ))
            .expect("shared Tier 3 query fixture");

        // Act
        workloads::prepare_tier3_query_domains(
            &context,
            TIER3_DOMAIN_TEST_ROWS,
            workloads::Tier3QueryDomains {
                join: true,
                graph: true,
                time_series: true,
            },
        )
        .expect("prepare every Tier 3 query domain in bounded transactions");

        // Assert
        assert_tier3_domain_generations_and_rows(&context);
        assert_tier3_domain_boundaries(&context);
        assert_tier3_join_boundary_semantics(&context);
        assert_tier3_graph_boundary_semantics(&context);
        assert_tier3_time_series_semantics(&context);
        let data_dir = context.data_dir.clone();
        context.cassie.shutdown();
        drop(context);
        std::fs::remove_dir_all(data_dir).expect("clean up batched Tier 3 query fixture");
    }

    #[test]
    fn should_attribute_tier3_domain_batch_errors_to_exact_range() {
        // Arrange
        let mut attempted = Vec::new();

        // Act
        let result = workloads::for_each_tier3_domain_batch(
            TIER3_DOMAIN_TEST_ROWS,
            "time-series events",
            |range| {
                attempted.push(range.clone());
                if range.start == workloads::BENCH_DOCUMENT_WRITE_BATCH_ROWS {
                    Err(cassie::app::CassieError::Storage(
                        "simulated storage deadline".to_string(),
                    ))
                } else {
                    Ok(())
                }
            },
        );

        // Assert
        assert_eq!(
            attempted,
            [
                0..workloads::BENCH_DOCUMENT_WRITE_BATCH_ROWS,
                workloads::BENCH_DOCUMENT_WRITE_BATCH_ROWS..TIER3_DOMAIN_TEST_ROWS,
            ]
        );
        assert_eq!(
            result.expect_err("second fixture batch should fail").to_string(),
            "storage error: prepare Tier 3 time-series events rows 5000..5002: storage error: simulated storage deadline"
        );
    }

    fn assert_tier3_domain_generations_and_rows(context: &workloads::BenchContext) {
        for collection in [
            "bench_join_users",
            "bench_join_orders",
            "bench_graph_nodes",
            "bench_graph_edges",
            "bench_time_series_events",
        ] {
            assert_eq!(
                context
                    .cassie
                    .midge
                    .collection_generation(collection)
                    .expect("read Tier 3 domain collection generation"),
                2,
                "{collection} must be loaded through exactly two fixture transactions"
            );
        }
        for (collection, expected_rows) in [
            ("bench_join_users", TIER3_DOMAIN_TEST_ROWS),
            ("bench_join_orders", TIER3_DOMAIN_TEST_ROWS),
            ("bench_graph_nodes", TIER3_DOMAIN_TEST_ROWS),
            ("bench_graph_edges", TIER3_DOMAIN_TEST_ROWS - 1),
            ("bench_time_series_events", TIER3_DOMAIN_TEST_ROWS),
        ] {
            assert_eq!(
                context
                    .cassie
                    .midge
                    .scan_documents(collection)
                    .expect("scan Tier 3 domain fixture")
                    .len(),
                expected_rows,
                "{collection} row count"
            );
        }
    }

    fn assert_tier3_domain_boundaries(context: &workloads::BenchContext) {
        for (collection, ids) in [
            (
                "bench_join_users",
                &["user-0", "user-4999", "user-5000", "user-5001"][..],
            ),
            (
                "bench_join_orders",
                &["order-0", "order-4999", "order-5000", "order-5001"][..],
            ),
            (
                "bench_graph_nodes",
                &["node-0", "node-4999", "node-5000", "node-5001"][..],
            ),
            (
                "bench_graph_edges",
                &["edge-0", "edge-4999", "edge-5000"][..],
            ),
            (
                "bench_time_series_events",
                &["ts-doc-0", "ts-doc-4999", "ts-doc-5000", "ts-doc-5001"][..],
            ),
        ] {
            for &id in ids {
                assert!(
                    context
                        .cassie
                        .midge
                        .get_document(collection, id)
                        .expect("read Tier 3 domain fixture boundary")
                        .is_some(),
                    "{collection} must retain boundary document {id}"
                );
            }
        }
    }

    fn assert_tier3_join_boundary_semantics(context: &workloads::BenchContext) {
        let user = context
            .cassie
            .midge
            .get_document("bench_join_users", "user-5000")
            .expect("read join user across fixture batch boundary")
            .expect("join user across fixture batch boundary");
        let order = context
            .cassie
            .midge
            .get_document("bench_join_orders", "order-5000")
            .expect("read join order across fixture batch boundary")
            .expect("join order across fixture batch boundary");
        assert!(
            context
                .cassie
                .catalog
                .get_index("bench_join_users", "bench_join_users_key_idx")
                .is_some(),
            "join index must remain registered after batched loading"
        );
        assert_eq!(user.payload["user_key"], serde_json::json!(5000));
        assert_eq!(user.payload["name"], serde_json::json!("user-5000"));
        assert_eq!(order.payload["order_user_key"], user.payload["user_key"]);
        assert_eq!(order.payload["total"], serde_json::json!(0));
    }

    fn assert_tier3_graph_boundary_semantics(context: &workloads::BenchContext) {
        let graph_before = context.cassie.metrics();
        let graph_rows = context
            .cassie
            .execute_sql(
                &context.session,
                TIER3_GRAPH_SQL,
                vec![
                    Value::String("bench_graph".to_string()),
                    Value::String("doc".to_string()),
                    Value::String("node-4998".to_string()),
                    Value::Int64(3),
                    Value::String("out".to_string()),
                    Value::String("links".to_string()),
                    Value::Int64(64),
                ],
            )
            .expect("expand graph across fixture batch boundary")
            .rows;
        let graph_after = context.cassie.metrics();
        assert_eq!(
            graph_rows,
            ["node-4999", "node-5000", "node-5001"]
                .into_iter()
                .map(|node| vec![Value::String(node.to_string())])
                .collect::<Vec<_>>()
        );
        assert_eq!(
            graph_after["graph"]["last_fallback_reason"],
            graph_before["graph"]["last_fallback_reason"],
            "batched graph fixture must retain native adjacency"
        );
    }

    fn assert_tier3_time_series_semantics(context: &workloads::BenchContext) {
        let time_series_params = || {
            vec![
                Value::String("2026-01-10T00:00:00Z".to_string()),
                Value::String("2026-01-12T00:00:00Z".to_string()),
            ]
        };
        workloads::assert_explain_contains(
            context,
            TIER3_TIME_SERIES_SQL,
            time_series_params(),
            "time_series_storage=bucket-native-v1",
        );
        let time_series_before = context.cassie.metrics();
        let time_series_rows = workloads::execute_expected_query(
            context,
            TIER3_TIME_SERIES_SQL,
            time_series_params(),
            512,
        );
        let time_series_after = context.cassie.metrics();
        assert_eq!(time_series_rows, 512);
        assert!(
            time_series_after["time_series"]["bucket_native_hits"]
                .as_u64()
                .unwrap_or_default()
                > time_series_before["time_series"]["bucket_native_hits"]
                    .as_u64()
                    .unwrap_or_default(),
            "time-series fixture must retain bucket-native reads"
        );
        assert_eq!(
            time_series_after["time_series"]["fallback_scans"].as_u64(),
            time_series_before["time_series"]["fallback_scans"].as_u64(),
            "batched time-series fixture must not fall back to row-backed reads"
        );
    }

    // Merged from tests/benchmark_join_fixture.rs to cut a separate test binary.
    #[test]
    fn should_expose_vectorized_join_fixture_to_sql() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        let context = runtime
            .block_on(workloads::vectorized_join_context(
                "join-fixture-sql-contract",
                4,
            ))
            .expect("join fixture");

        // Act
        let result = context.cassie.execute_sql(
            &context.session,
            "SELECT bench_join_users.name FROM bench_join_users JOIN bench_join_orders ON bench_join_users.user_key = bench_join_orders.order_user_key LIMIT 1",
            vec![],
        );

        // Assert
        assert!(
            result.is_ok(),
            "join fixture must be SQL-visible: {result:?}"
        );
    }
}
// Formerly tests/operational_smoke.rs.
mod operational_smoke {
    #![cfg(unix)]

    use std::path::Path;
    use std::process::{Command as StdCommand, Stdio};
    use std::time::Duration;

    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio::process::{Child, Command};
    use uuid::Uuid;

    const ADMIN_PASSWORD: &str = "postgres";

    struct JsonHttpResponse {
        status: u16,
        body: serde_json::Value,
        session_cookie: Option<String>,
    }

    impl JsonHttpResponse {
        fn is_success(&self) -> bool {
            (200..300).contains(&self.status)
        }
    }

    use super::support_data_dir as data_dir;
    use data_dir::data_dir;

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind ephemeral port")
            .local_addr()
            .expect("local address")
            .port()
    }

    fn binary_path() -> &'static str {
        env!("CARGO_BIN_EXE_cassie")
    }

    fn spawn_cassie(data_dir: &Path, rest_port: u16, pgwire_port: u16) -> Child {
        let mut command = Command::new(binary_path());
        command
            .env("CASSIE_STORAGE_MODE", "local")
            .env("CASSIE_STORAGE_PATH", data_dir)
            .env("CASSIE_REST_LISTEN", format!("127.0.0.1:{rest_port}"))
            .env("CASSIE_PGWIRE_LISTEN", format!("127.0.0.1:{pgwire_port}"))
            .env("CASSIE_DEFAULT_DATABASE", "postgres")
            .env("CASSIE_ROOT_PASSWORD", ADMIN_PASSWORD)
            .env("CASSIE_EMBEDDINGS_PROVIDER", "disabled")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command.kill_on_drop(true);
        command.spawn().expect("spawn cassie binary")
    }

    async fn wait_for_ready(child: &mut Child, base_url: &str) {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                if let Some(status) = child.try_wait().expect("poll cassie child") {
                    panic!("cassie exited before becoming ready: {status}");
                }

                if let Ok(response) = request_json("GET", base_url, "/health", None, None).await {
                    if response.is_success() && response.body["ready"].as_bool() == Some(true) {
                        assert_eq!(response.body["status"], "ok");
                        break;
                    }
                }

                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("cassie should become ready");
    }

    async fn request_json(
        method: &str,
        base_url: &str,
        path: &str,
        body: Option<serde_json::Value>,
        session_cookie: Option<&str>,
    ) -> Result<JsonHttpResponse, String> {
        let (host, port) = parse_localhost_url(base_url)?;
        let mut stream = TcpStream::connect((host.as_str(), port))
            .await
            .map_err(|error| format!("connect {base_url}: {error}"))?;
        let body = body
            .map(|value| serde_json::to_vec(&value).map_err(|error| error.to_string()))
            .transpose()?
            .unwrap_or_default();
        let content_type = if body.is_empty() {
            ""
        } else {
            "Content-Type: application/json\r\n"
        };
        let cookie = session_cookie
            .map(|value| format!("Cookie: {value}\r\n"))
            .unwrap_or_default();
        let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n{cookie}{content_type}Content-Length: {}\r\n\r\n",
        body.len()
    );

        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|error| format!("write request: {error}"))?;
        stream
            .write_all(&body)
            .await
            .map_err(|error| format!("write body: {error}"))?;

        let mut raw_response = Vec::new();
        stream
            .read_to_end(&mut raw_response)
            .await
            .map_err(|error| format!("read response: {error}"))?;
        parse_json_response(&raw_response)
    }

    fn parse_localhost_url(base_url: &str) -> Result<(String, u16), String> {
        let host_port = base_url
            .strip_prefix("http://")
            .ok_or_else(|| format!("unsupported base URL '{base_url}'"))?;
        let (host, port) = host_port
            .rsplit_once(':')
            .ok_or_else(|| format!("missing port in base URL '{base_url}'"))?;
        let port = port
            .parse::<u16>()
            .map_err(|error| format!("invalid port in base URL '{base_url}': {error}"))?;
        Ok((host.to_string(), port))
    }

    fn parse_json_response(raw_response: &[u8]) -> Result<JsonHttpResponse, String> {
        let response =
            std::str::from_utf8(raw_response).map_err(|error| format!("response utf8: {error}"))?;
        let (head, body) = response
            .split_once("\r\n\r\n")
            .ok_or_else(|| "response missing header terminator".to_string())?;
        let status = head
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .ok_or_else(|| "response missing status".to_string())?
            .parse::<u16>()
            .map_err(|error| format!("invalid status: {error}"))?;
        let session_cookie = head.lines().skip(1).find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("set-cookie").then(|| {
                value
                    .trim()
                    .split(';')
                    .next()
                    .unwrap_or_default()
                    .to_string()
            })
        });
        let body = serde_json::from_str(body).map_err(|error| format!("response json: {error}"))?;
        Ok(JsonHttpResponse {
            status,
            body,
            session_cookie,
        })
    }

    async fn rest_login(base_url: &str) -> String {
        let response = request_json(
            "POST",
            base_url,
            "/api/v1/auth/login",
            Some(json!({
                "username": "root",
                "password": ADMIN_PASSWORD,
            })),
            None,
        )
        .await
        .expect("REST login request");
        assert!(response.is_success(), "REST login should succeed");
        response.session_cookie.expect("REST session cookie")
    }

    async fn terminate_cleanly(child: &mut Child) {
        let pid = child.id().expect("child pid");
        let status = StdCommand::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .status()
            .expect("send SIGTERM");
        assert!(status.success(), "SIGTERM should be delivered successfully");

        let exit_status = tokio::time::timeout(Duration::from_secs(10), child.wait())
            .await
            .expect("cassie should exit after SIGTERM")
            .expect("wait for cassie child");
        assert!(
            exit_status.success(),
            "cassie should exit cleanly after SIGTERM"
        );
    }

    async fn pgwire_query_one_text(port: u16, sql: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect pgwire");
        write_pg_startup(&mut stream).await;
        wait_for_pg_ready(&mut stream).await;
        write_pg_query(&mut stream, sql).await;
        let value = read_pg_query_text(&mut stream).await;
        write_pg_terminate(&mut stream).await;
        value
    }

    async fn write_pg_startup(stream: &mut TcpStream) {
        let mut payload = Vec::new();
        payload.extend_from_slice(&196_608_i32.to_be_bytes());
        payload.extend_from_slice(b"user\0root\0database\0postgres\0\0");
        write_pg_untagged(stream, &payload).await;
    }

    async fn write_pg_query(stream: &mut TcpStream, sql: &str) {
        let mut payload = Vec::from(sql.as_bytes());
        payload.push(0);
        write_pg_tagged(stream, b'Q', &payload).await;
    }

    async fn write_pg_password(stream: &mut TcpStream) {
        let mut payload = Vec::from(ADMIN_PASSWORD.as_bytes());
        payload.push(0);
        write_pg_tagged(stream, b'p', &payload).await;
    }

    async fn write_pg_terminate(stream: &mut TcpStream) {
        write_pg_tagged(stream, b'X', &[]).await;
    }

    async fn write_pg_untagged(stream: &mut TcpStream, payload: &[u8]) {
        let length = i32::try_from(payload.len() + 4).expect("pgwire message length");
        stream
            .write_all(&length.to_be_bytes())
            .await
            .expect("write pgwire length");
        stream
            .write_all(payload)
            .await
            .expect("write pgwire payload");
    }

    async fn write_pg_tagged(stream: &mut TcpStream, tag: u8, payload: &[u8]) {
        stream.write_all(&[tag]).await.expect("write pgwire tag");
        write_pg_untagged(stream, payload).await;
    }

    async fn wait_for_pg_ready(stream: &mut TcpStream) {
        loop {
            let (tag, payload) = read_pg_message(stream).await;
            match tag {
                b'R' => match read_i32(&payload, 0) {
                    0 => {}
                    3 => write_pg_password(stream).await,
                    code => panic!("unsupported pgwire authentication code: {code}"),
                },
                b'E' => panic!("pgwire startup error: {}", pg_error_message(&payload)),
                b'Z' => break,
                _ => {}
            }
        }
    }

    async fn read_pg_query_text(stream: &mut TcpStream) -> String {
        let mut first_value = None;
        loop {
            let (tag, payload) = read_pg_message(stream).await;
            match tag {
                b'D' => {
                    if first_value.is_none() {
                        first_value = Some(read_first_data_row_value(&payload));
                    }
                }
                b'E' => panic!("pgwire query error: {}", pg_error_message(&payload)),
                b'Z' => return first_value.expect("pgwire row"),
                _ => {}
            }
        }
    }

    async fn read_pg_message(stream: &mut TcpStream) -> (u8, Vec<u8>) {
        let mut tag = [0_u8; 1];
        stream.read_exact(&mut tag).await.expect("read pgwire tag");
        let mut length = [0_u8; 4];
        stream
            .read_exact(&mut length)
            .await
            .expect("read pgwire length");
        let payload_len = u32::from_be_bytes(length)
            .checked_sub(4)
            .expect("pgwire payload length");
        let payload_len = usize::try_from(payload_len).expect("pgwire payload length usize");
        let mut payload = vec![0_u8; payload_len];
        stream
            .read_exact(&mut payload)
            .await
            .expect("read pgwire payload");
        (tag[0], payload)
    }

    fn read_first_data_row_value(payload: &[u8]) -> String {
        let mut offset = 0;
        assert_eq!(read_u16_at(payload, &mut offset), 1, "pgwire column count");
        let length = read_i32_at(payload, &mut offset);
        assert!(length >= 0, "pgwire value should not be null");
        let length = usize::try_from(length).expect("pgwire value length");
        let end = offset + length;
        let value = payload.get(offset..end).expect("pgwire value bytes");
        std::str::from_utf8(value)
            .expect("pgwire value utf8")
            .to_string()
    }

    fn read_u16_at(payload: &[u8], offset: &mut usize) -> u16 {
        let end = *offset + 2;
        let bytes = payload.get(*offset..end).expect("pgwire u16");
        *offset = end;
        u16::from_be_bytes(bytes.try_into().expect("pgwire u16 bytes"))
    }

    fn read_i32_at(payload: &[u8], offset: &mut usize) -> i32 {
        let value = read_i32(payload, *offset);
        *offset += 4;
        value
    }

    fn read_i32(payload: &[u8], offset: usize) -> i32 {
        let end = offset + 4;
        let bytes = payload.get(offset..end).expect("pgwire i32");
        i32::from_be_bytes(bytes.try_into().expect("pgwire i32 bytes"))
    }

    fn pg_error_message(payload: &[u8]) -> String {
        let fields = payload
            .split(|byte| *byte == 0)
            .filter(|field| !field.is_empty())
            .map(|field| String::from_utf8_lossy(field).into_owned())
            .collect::<Vec<_>>();
        fields.join("; ")
    }

    #[test]
    fn should_expose_kubernetes_style_health_probes_through_the_binary() {
        // Arrange
        let path = data_dir("startup");
        let rest_port = free_port();
        let pgwire_port = free_port();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut child = spawn_cassie(&path, rest_port, pgwire_port);
            let base_url = format!("http://127.0.0.1:{rest_port}");

            // Act
            wait_for_ready(&mut child, &base_url).await;
            for path in ["/healthz", "/readyz", "/livez", "/startupz"] {
                let probe = request_json("GET", &base_url, path, None, None)
                    .await
                    .expect("probe request");
                assert!(probe.is_success(), "probe failed: {path}");
                assert_eq!(probe.body["ready"].as_bool(), Some(true));
            }

            // Assert
            terminate_cleanly(&mut child).await;
            let _ = std::fs::remove_dir_all(&path);
        });
    }

    #[test]
    fn should_restart_with_hydrated_catalog_through_the_binary() {
        // Arrange
        let path = data_dir("restart");
        let rest_port = free_port();
        let pgwire_port = free_port();
        let collection = format!("smoke_docs_{}", Uuid::new_v4().simple());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let base_url = format!("http://127.0.0.1:{rest_port}");

            let mut child = spawn_cassie(&path, rest_port, pgwire_port);
            wait_for_ready(&mut child, &base_url).await;
            let session_cookie = rest_login(&base_url).await;

            // Act
            let create = request_json(
                "POST",
                &base_url,
                "/api/v1/collections",
                Some(json!({
                    "name": collection,
                    "fields": [
                        {"name": "title", "type": "text"}
                    ]
                })),
                Some(&session_cookie),
            )
            .await
            .expect("create collection request");
            assert!(create.is_success());
            assert_eq!(create.body["collection"], collection);

            let document = request_json(
                "POST",
                &base_url,
                &format!("/api/v1/collections/{collection}/documents"),
                Some(json!({"title": "alpha"})),
                Some(&session_cookie),
            )
            .await
            .expect("create document request");
            assert!(document.is_success());
            let document_id = document.body["id"]
                .as_str()
                .expect("document id present")
                .to_string();

            terminate_cleanly(&mut child).await;

            let mut child = spawn_cassie(&path, rest_port, pgwire_port);
            wait_for_ready(&mut child, &base_url).await;

            let title = tokio::time::timeout(
                Duration::from_secs(5),
                pgwire_query_one_text(
                    pgwire_port,
                    &format!("SELECT title FROM {collection} ORDER BY title"),
                ),
            )
            .await
            .expect("pgwire query should complete");

            let get = request_json(
                "GET",
                &base_url,
                &format!("/api/v1/collections/{collection}/documents/{document_id}"),
                None,
                Some(&session_cookie),
            )
            .await
            .expect("get document request");
            assert!(get.is_success());

            // Assert
            assert_eq!(title, "alpha");
            assert_eq!(get.body["title"], "alpha");

            terminate_cleanly(&mut child).await;
            let _ = std::fs::remove_dir_all(&path);
        });
    }
}

// Formerly tests/performance_benchmarks.rs.
mod performance_benchmarks_tests {
    use super::performance_benchmarks;

    use std::path::{Path, PathBuf};

    use performance_benchmarks::{
        benchmark_for_benchmark, benchmark_scenarios, deployment_profile_for_id,
        expected_stress_artifact_path, summarize_stress_artifact, summarize_stress_artifact_rows,
        validate_stress_artifact_signal_metadata, BenchmarkTier, BenchmarkTimingMode, FixtureClass,
        PerformanceBenchmarkScenario, ResultCachePolicy, REQUIRED_WORKLOAD_FAMILIES,
    };

    #[test]
    fn should_parse_stress_artifact_percentiles() {
        // Arrange
        let benchmark = PerformanceBenchmarkScenario {
            scenario_id: "test.scenario",
            family: "core_read",
            access_family: "relational_index",
            benchmark: "tier3_system_query",
            workload: "mixed_order_scalar_query",
            fixture_scale: "100k",
            fixture_rows: 100_000,
            declared_tier: BenchmarkTier::Tier3,
            timing_mode: BenchmarkTimingMode::Batch,
            operation_unit: "query",
            evidence_role: performance_benchmarks::EvidenceRole::Gate,
            fixture_class: FixtureClass::Representative,
            result_cache_policy: ResultCachePolicy::Disabled,
            client_count: None,
            worker_count: None,
            memory_evidence: "storage.data.reads",
            fallback_evidence: "fallback_reason",
            explain_evidence: "access_path",
            metrics_evidence: "query.latency_ms_total",
        };
        let artifact = r#"{
            "schema_version": "cntryl-stress.v1",
            "summaries": [{
                "benchmark_id": "tier3_system_query/mixed_order_scalar_query/100k",
                "primary_metric": "throughput",
                "stats": {
                    "mean": 500000.0,
                    "p50": 490000.0,
                    "p95": 505000.0,
                    "p99": 510000.0
                },
                "ns_per_op": {
                    "mean": 2000.0,
                    "p50": 2000.0,
                    "p95": 3000.0,
                    "p99": 3000.0
                },
                "metadata": {
                    "scenario_id": "test.scenario",
                    "family": "core_read",
                    "benchmark": "tier3_system_query",
                    "workload": "mixed_order_scalar_query",
                    "fixture_scale": "100k"
                }
            }]
        }"#;

        // Act
        let summary = summarize_stress_artifact(&benchmark, artifact).expect("stress summary");

        // Assert
        assert_eq!(summary.scenario_id, "test.scenario");
        assert_eq!(summary.profile_id, "local-dev-fallback-100k");
        assert_eq!(summary.p50_us, 2);
        assert_eq!(summary.p95_us, 3);
        assert_eq!(summary.p99_us, 3);
        assert!((summary.throughput_ops_per_sec - 500_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn should_reject_wrong_stress_schema_version() {
        // Arrange
        let benchmark =
            benchmark_for_benchmark("tier3_system_query", "mixed_order_scalar_query", "100k")
                .expect("query benchmark");
        let artifact = r#"{
            "schema_version": "cntryl-stress.v999",
            "summaries": []
        }"#;

        // Act
        let error = summarize_stress_artifact(benchmark, artifact).expect_err("schema error");

        // Assert
        assert!(error.contains("unsupported schema_version"));
    }

    #[test]
    fn should_parse_cntryl_stress_v2_artifacts() {
        // Arrange
        let benchmark = PerformanceBenchmarkScenario {
            scenario_id: "test.scenario",
            family: "core_read",
            access_family: "relational_index",
            benchmark: "tier3_system_query",
            workload: "mixed_order_scalar_query",
            fixture_scale: "100k",
            fixture_rows: 100_000,
            declared_tier: BenchmarkTier::Tier3,
            timing_mode: BenchmarkTimingMode::Batch,
            operation_unit: "query",
            evidence_role: performance_benchmarks::EvidenceRole::Gate,
            fixture_class: FixtureClass::Representative,
            result_cache_policy: ResultCachePolicy::Disabled,
            client_count: None,
            worker_count: None,
            memory_evidence: "storage.data.reads",
            fallback_evidence: "fallback_reason",
            explain_evidence: "access_path",
            metrics_evidence: "query.latency_ms_total",
        };
        let artifact = r#"{
            "schema_version": "cntryl-stress.v2",
            "summaries": [{
                "benchmark_id": "tier3_system_query/mixed_order_scalar_query/100k/mixed_order_scalar_query/100k",
                "name": "mixed_order_scalar_query/100k",
                "tier": 3,
                "intent": "batch",
                "primary_metric": "throughput",
                "stats": {
                    "mean": 500000.0,
                    "p50": 490000.0,
                    "p95": 505000.0,
                    "p99": 510000.0
                },
                "ns_per_op": {
                    "mean": 2000.0,
                    "p50": 2000.0,
                    "p95": 3000.0,
                    "p99": 3000.0
                },
                "diagnostics": [],
                "metadata": {
                    "scenario_id": "test.scenario",
                    "family": "core_read",
                    "benchmark": "tier3_system_query",
                    "workload": "mixed_order_scalar_query",
                    "fixture_scale": "100k",
                    "operation_unit": "query",
                    "logical_operations_per_iteration": "64"
                }
            }]
        }"#;

        // Act
        let summary = summarize_stress_artifact(&benchmark, artifact).expect("stress summary");

        // Assert
        assert_eq!(summary.scenario_id, "test.scenario");
        assert_eq!(summary.p50_us, 2);
        assert_eq!(summary.p95_us, 3);
        assert_eq!(summary.p99_us, 3);
        assert!((summary.throughput_ops_per_sec - 500_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn should_normalize_nullable_stress_diagnostics() {
        // Arrange
        let artifact = r#"{
            "schema_version": "cntryl-stress.v2",
            "diagnostics_summary": null,
            "summaries": [{
                "benchmark_id": "tier2_subsystem_parser/sql_parser/10k/sql_parser/10k",
                "name": "sql_parser/10k",
                "tier": 2,
                "intent": "batch",
                "primary_metric": "throughput",
                "stats": { "mean": 1000.0, "p50": 1000.0, "p95": 1000.0, "p99": 1000.0 },
                "ns_per_op": { "mean": 1000.0, "p50": 1000.0, "p95": 1000.0, "p99": 1000.0 },
                "diagnostics": null,
                "metadata": {
                    "scenario_id": "perf.sql.parser.10k",
                    "family": "core_read",
                    "benchmark": "tier2_subsystem_parser",
                    "workload": "sql_parser",
                    "fixture_scale": "10k",
                    "operation_unit": "sql_statement",
                    "logical_operations_per_iteration": "256"
                }
            }]
        }"#;

        // Act
        let rows = summarize_stress_artifact_rows(artifact).expect("stress rows");

        // Assert
        assert_eq!(rows.len(), 1);
        assert!(rows[0].diagnostic_codes.is_empty());
    }

    #[test]
    fn should_validate_unique_scenario_ids_with_known_families() {
        // Arrange
        let mut scenario_ids = std::collections::BTreeSet::new();
        let families = REQUIRED_WORKLOAD_FAMILIES
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();

        // Act
        let invalid = benchmark_scenarios()
            .filter(|scenario| {
                !scenario_ids.insert(scenario.scenario_id) || !families.contains(scenario.family)
            })
            .collect::<Vec<_>>();

        // Assert
        assert!(
            invalid.is_empty(),
            "duplicate scenario ids or invalid families: {invalid:?}"
        );
    }

    const PRESERVED_SCENARIO_IDS: &[(&str, &str, &str, &str)] = &[
        (
            "tier4_integration_pgwire",
            "simple_query",
            "10k",
            "perf.pgwire.simple_query.10k",
        ),
        (
            "tier4_integration_pgwire",
            "multi_statement",
            "10k",
            "perf.pgwire.multi_statement_query.10k",
        ),
        (
            "tier4_integration_pgwire",
            "binary_extended_query",
            "10k",
            "perf.pgwire.binary_query.10k",
        ),
        (
            "tier4_integration_http",
            "document_create_get",
            "10k",
            "perf.http.document_create_get.10k",
        ),
        (
            "tier4_integration_http",
            "vector_search",
            "10k",
            "perf.http.vector_search.10k",
        ),
        (
            "tier4_integration_protocol_compare",
            "pgwire_query",
            "10k",
            "perf.protocol_compare.pgwire_query.10k",
        ),
        (
            "tier4_integration_protocol_compare",
            "http_query",
            "10k",
            "perf.protocol_compare.http_json_query.10k",
        ),
        (
            "tier5_scaling_lifecycle",
            "projection_replay",
            "10k",
            "perf.replay.lag_catchup.10k",
        ),
        (
            "tier5_scaling_lifecycle",
            "projection_replay",
            "100k",
            "perf.replay.lag_catchup.100k",
        ),
        (
            "tier5_scaling_lifecycle",
            "projection_rebuild",
            "10k",
            "perf.rebuild.refresh.10k",
        ),
        (
            "tier5_scaling_lifecycle",
            "projection_rebuild",
            "100k",
            "perf.rebuild.refresh.100k",
        ),
        (
            "tier5_scaling_lifecycle",
            "projection_replay",
            "250k",
            "perf.scale.replay.lag_catchup.250k",
        ),
        (
            "tier5_scaling_lifecycle",
            "projection_rebuild",
            "250k",
            "perf.scale.rebuild.refresh.250k",
        ),
        (
            "tier5_scaling_query",
            "simple_sql_query",
            "100k",
            "perf.core_read.simple.100k",
        ),
        (
            "tier5_scaling_query",
            "recursive_cte_query",
            "100k",
            "perf.core_read.recursive_cte.100k",
        ),
        (
            "tier5_scaling_query",
            "window_frame_query",
            "100k",
            "perf.core_read.window_frames.100k",
        ),
        (
            "tier5_scaling_query",
            "mixed_direction_scalar_query",
            "100k",
            "perf.read_path.mixed_direction_suffix.100k",
        ),
        (
            "tier5_scaling_query",
            "expression_index_query",
            "100k",
            "perf.read_path.expression_index.100k",
        ),
        (
            "tier5_scaling_query",
            "expression_index_range_query",
            "100k",
            "perf.read_path.expression_index_range.100k",
        ),
        (
            "tier5_scaling_query",
            "expression_index_order_query",
            "100k",
            "perf.read_path.expression_index_order.100k",
        ),
        (
            "tier5_scaling_query",
            "vectorized_left_join_limited",
            "100k",
            "perf.read_path.vectorized_left_join_limited.100k",
        ),
        (
            "tier5_scaling_query",
            "vectorized_streaming_inner_join",
            "100k",
            "perf.read_path.vectorized_streaming_inner_join.100k",
        ),
        (
            "tier5_scaling_query",
            "vectorized_dense_streaming_inner_join",
            "100k",
            "perf.read_path.vectorized_dense_streaming_inner_join.100k",
        ),
        (
            "tier5_scaling_query",
            "vectorized_indexed_inner_join",
            "100k",
            "perf.read_path.vectorized_indexed_inner_join.100k",
        ),
        (
            "tier5_scaling_query",
            "vectorized_right_indexed_inner_join",
            "100k",
            "perf.read_path.vectorized_right_indexed_inner_join.100k",
        ),
        (
            "tier5_scaling_query",
            "vectorized_late_match_inner_join",
            "100k",
            "perf.read_path.vectorized_late_match_inner_join.100k",
        ),
        (
            "tier5_scaling_query",
            "vectorized_fanout_inner_join",
            "100k",
            "perf.read_path.vectorized_fanout_inner_join.100k",
        ),
        (
            "tier5_scaling_retrieval",
            "full_text_cold",
            "100k",
            "perf.search.fulltext_cold.100k",
        ),
        (
            "tier5_scaling_retrieval",
            "full_text_warm",
            "100k",
            "perf.search.fulltext_warm.100k",
        ),
        (
            "tier5_scaling_retrieval",
            "full_text_cold",
            "250k",
            "perf.search.fulltext_cold.250k",
        ),
        (
            "tier5_scaling_retrieval",
            "full_text_warm",
            "250k",
            "perf.search.fulltext_warm.250k",
        ),
        (
            "tier5_scaling_retrieval",
            "vector_hnsw_persisted",
            "250k",
            "perf.vector.hnsw_persisted.250k",
        ),
        (
            "tier5_scaling_retrieval",
            "vector_ivfflat_persisted",
            "250k",
            "perf.vector.ivfflat_persisted.250k",
        ),
        (
            "tier5_scaling_retrieval",
            "hybrid_query",
            "250k",
            "perf.hybrid.executor.250k",
        ),
        (
            "tier5_scaling_lifecycle",
            "time_series_retention_enforcement",
            "100k",
            "perf.time_series.retention.100k",
        ),
        (
            "tier5_scaling_lifecycle",
            "time_series_rollup_refresh",
            "100k",
            "perf.time_series.rollup_refresh.100k",
        ),
        (
            "tier5_scaling_lifecycle",
            "projection_verify",
            "100k",
            "perf.verification.full.100k",
        ),
        (
            "tier5_scaling_transport",
            "pgwire_simple_query",
            "100k",
            "perf.pgwire.simple_query.100k",
        ),
        (
            "tier5_scaling_transport",
            "pgwire_multi_statement_query",
            "100k",
            "perf.pgwire.multi_statement_query.100k",
        ),
        (
            "tier5_scaling_transport",
            "pgwire_binary_query",
            "100k",
            "perf.pgwire.binary_query.100k",
        ),
        (
            "tier5_scaling_transport",
            "pgwire_prepared_query",
            "100k",
            "perf.pgwire.prepared_query.100k",
        ),
        (
            "tier5_scaling_transport",
            "http_document_create_get",
            "100k",
            "perf.http.document_create_get.100k",
        ),
    ];

    #[test]
    fn should_preserve_scenario_ids_when_tier_ownership_moves() {
        // Arrange
        let expected = PRESERVED_SCENARIO_IDS;

        // Act
        let mismatches = expected
            .iter()
            .copied()
            .filter_map(|(owner, workload, scale, scenario_id)| {
                let registered = benchmark_for_benchmark(owner, workload, scale);
                (registered.map(|scenario| scenario.scenario_id) != Some(scenario_id)).then_some((
                    owner,
                    workload,
                    scale,
                    scenario_id,
                    registered.map(|scenario| scenario.scenario_id),
                ))
            })
            .collect::<Vec<_>>();

        // Assert
        assert!(
            mismatches.is_empty(),
            "scenario IDs changed across tier ownership moves: {mismatches:?}"
        );
    }

    #[test]
    fn should_keep_future_scale_placeholders_out_of_runnable_scenarios() {
        // Arrange
        const FUTURE_PROFILE_ID: &str = "future-1m-placeholder";

        // Act
        let future_scale = benchmark_scenarios()
            .filter(|scenario| scenario.fixture_scale == "1M")
            .map(|scenario| scenario.scenario_id)
            .collect::<Vec<_>>();

        // Assert
        assert!(
            future_scale.is_empty(),
            "future 1M placeholders must not be runnable benchmark scenarios: {future_scale:?}"
        );
        assert!(
            deployment_profile_for_id(FUTURE_PROFILE_ID).is_none(),
            "future 1M scale should stay docs-only until a runnable fixture exists"
        );
    }

    #[test]
    fn should_assign_persisted_ann_across_required_tiers() {
        // Arrange
        let required = [
            ("tier3_system_query", "vector_hnsw_persisted", "100k"),
            ("tier3_system_query", "vector_ivfflat_persisted", "100k"),
            ("tier5_scaling_retrieval", "vector_hnsw_persisted", "10k"),
            ("tier5_scaling_retrieval", "vector_hnsw_persisted", "250k"),
            ("tier5_scaling_retrieval", "vector_ivfflat_persisted", "10k"),
            (
                "tier5_scaling_retrieval",
                "vector_ivfflat_persisted",
                "250k",
            ),
        ];

        // Act
        let missing = required
            .into_iter()
            .filter(|(owner, workload, scale)| {
                benchmark_for_benchmark(owner, workload, scale).is_none()
            })
            .collect::<Vec<_>>();

        // Assert
        assert!(
            missing.is_empty(),
            "missing persisted ANN scenarios: {missing:?}"
        );
    }

    #[test]
    fn should_register_retrieval_scaling_scenarios_at_required_scales() {
        // Arrange
        let required_workloads = [
            "full_text_query",
            "vector_exact_query",
            "vector_hnsw_persisted",
            "vector_ivfflat_persisted",
            "hybrid_query",
        ];
        let required_scales = ["10k", "100k", "250k"];

        // Act
        let missing = required_workloads
            .into_iter()
            .flat_map(|workload| {
                required_scales.into_iter().filter_map(move |scale| {
                    benchmark_for_benchmark("tier5_scaling_retrieval", workload, scale)
                        .is_none()
                        .then_some((workload, scale))
                })
            })
            .collect::<Vec<_>>();

        // Assert
        assert!(
            missing.is_empty(),
            "missing full-text temperature scenarios: {missing:?}"
        );
    }

    #[test]
    fn should_enforce_registered_tier_contracts() {
        // Arrange
        let scenarios = benchmark_scenarios().collect::<Vec<_>>();

        // Act
        let failures = scenarios
            .iter()
            .filter_map(|scenario| {
                performance_benchmarks::validate_scenario_contract(scenario)
                    .err()
                    .map(|error| (scenario.scenario_id, error))
            })
            .collect::<Vec<_>>();

        // Assert
        assert!(
            failures.is_empty(),
            "invalid benchmark scenarios: {failures:?}"
        );
        assert!(scenarios
            .iter()
            .all(|scenario| match scenario.declared_tier {
                BenchmarkTier::Tier1 => scenario.timing_mode == BenchmarkTimingMode::Micro,
                BenchmarkTier::Tier2 => matches!(
                    scenario.timing_mode,
                    BenchmarkTimingMode::Measure | BenchmarkTimingMode::Counted
                ),
                BenchmarkTier::Tier3 | BenchmarkTier::Tier5 => {
                    scenario.timing_mode == BenchmarkTimingMode::Batch
                }
                BenchmarkTier::Tier4 | BenchmarkTier::Tier6 => matches!(
                    scenario.timing_mode,
                    BenchmarkTimingMode::Batch | BenchmarkTimingMode::External
                ),
            }));
    }

    #[test]
    fn should_cap_tier2_fixtures_at_2048_rows() {
        // Arrange
        let tier2 = benchmark_scenarios()
            .filter(|scenario| scenario.declared_tier == BenchmarkTier::Tier2)
            .collect::<Vec<_>>();

        // Act
        let oversized = tier2
            .iter()
            .filter(|scenario| scenario.fixture_rows > 2_048)
            .map(|scenario| (scenario.scenario_id, scenario.fixture_rows))
            .collect::<Vec<_>>();

        // Assert
        assert!(!tier2.is_empty());
        assert!(
            oversized.is_empty(),
            "oversized Tier 2 fixtures: {oversized:?}"
        );
    }

    #[test]
    fn should_match_tier1_units_to_one_timed_kernel_invocation() {
        // Arrange
        let expected_units = [
            ("tier1_hotpath_filter_projection", "batch_filter", "batch"),
            (
                "tier1_hotpath_filter_projection",
                "value_comparison",
                "comparison",
            ),
            ("tier1_hotpath_search_vector", "tokenization", "text"),
            (
                "tier1_hotpath_topk",
                "top_k_heap_maintenance",
                "top_k_maintenance",
            ),
            (
                "tier1_hotpath_vector_distance",
                "cosine_distance",
                "distance",
            ),
            ("tier1_hotpath_bm25", "bm25_scoring", "score"),
        ];

        // Act
        let actual_units = expected_units.map(|(owner, workload, expected)| {
            let scenario = benchmark_for_benchmark(owner, workload, "micro")
                .expect("registered Tier 1 scenario");
            (scenario.operation_unit, expected)
        });

        // Assert
        assert!(actual_units
            .into_iter()
            .all(|(actual, expected)| actual == expected));
    }

    #[test]
    fn should_keep_tier3_to_one_100k_representative_per_access_family() {
        // Arrange
        let required_families = [
            "relational_index",
            "join",
            "column_analytics",
            "fulltext",
            "vector_exact",
            "vector_hnsw",
            "vector_ivf",
            "hybrid",
            "graph",
            "time_series",
            "lifecycle",
            "mixed_load",
        ];

        // Act
        let tier3 = benchmark_scenarios()
            .filter(|scenario| scenario.declared_tier == BenchmarkTier::Tier3)
            .collect::<Vec<_>>();
        let counts = required_families
            .into_iter()
            .map(|family| {
                let count = tier3
                    .iter()
                    .filter(|scenario| scenario.access_family == family)
                    .count();
                (family, count)
            })
            .collect::<Vec<_>>();

        // Assert
        assert!(tier3.iter().all(|scenario| {
            scenario.fixture_class == FixtureClass::Representative
                && scenario.fixture_rows == 100_000
        }));
        assert!(
            counts.iter().all(|(_, count)| *count == 1),
            "Tier 3 representative counts: {counts:?}"
        );
    }

    #[test]
    fn should_register_complete_tier5_sweep_axes() {
        // Arrange
        let tier5 = benchmark_scenarios()
            .filter(|scenario| scenario.declared_tier == BenchmarkTier::Tier5)
            .collect::<Vec<_>>();

        // Act
        let scales = tier5
            .iter()
            .map(|scenario| scenario.fixture_rows)
            .collect::<std::collections::BTreeSet<_>>();
        let clients = tier5
            .iter()
            .filter_map(|scenario| scenario.client_count)
            .collect::<std::collections::BTreeSet<_>>();
        let workers = tier5
            .iter()
            .filter_map(|scenario| scenario.worker_count)
            .collect::<std::collections::BTreeSet<_>>();

        // Assert
        assert!(scales.is_superset(&[10_000, 100_000, 250_000].into_iter().collect()));
        assert_eq!(clients, [1, 2, 4, 8, 16].into_iter().collect());
        assert_eq!(workers, [1, 2, 4].into_iter().collect());
    }

    #[test]
    fn should_register_exactly_two_tier6_endurance_scenarios() {
        // Arrange
        let tier6 = benchmark_scenarios()
            .filter(|scenario| scenario.declared_tier == BenchmarkTier::Tier6)
            .collect::<Vec<_>>();

        // Act
        let shapes = tier6
            .iter()
            .map(|scenario| (scenario.access_family, scenario.fixture_rows))
            .collect::<std::collections::BTreeSet<_>>();

        // Assert
        assert_eq!(tier6.len(), 2);
        assert_eq!(
            shapes,
            [("mixed_load", 100_000), ("transport_lifecycle", 10_000)]
                .into_iter()
                .collect()
        );
        assert!(tier6
            .iter()
            .all(|scenario| scenario.fixture_class == FixtureClass::Soak));
    }

    #[test]
    fn should_enable_execution_result_cache_only_for_dedicated_tier2_case() {
        // Arrange
        let cache_owners = benchmark_scenarios()
            .filter(|scenario| scenario.result_cache_policy == ResultCachePolicy::Measured)
            .collect::<Vec<_>>();

        // Act
        let owner = cache_owners.first().copied();

        // Assert
        assert_eq!(cache_owners.len(), 1);
        assert_eq!(
            owner.map(|scenario| scenario.declared_tier),
            Some(BenchmarkTier::Tier2)
        );
        assert_eq!(
            owner.map(|scenario| (scenario.benchmark, scenario.workload)),
            Some(("tier2_subsystem_plan_cache", "execution_result_cache_hit"))
        );
    }

    #[test]
    fn should_register_every_scenario_owner_in_cargo_manifest() {
        // Arrange
        let manifest = include_str!("../Cargo.toml");
        let registered = manifest
            .lines()
            .filter_map(|line| {
                line.trim()
                    .strip_prefix("name = \"")
                    .and_then(|value| value.strip_suffix('"'))
                    .filter(|name| name.starts_with("tier"))
            })
            .collect::<std::collections::BTreeSet<_>>();

        // Act
        let owners = benchmark_scenarios()
            .map(|scenario| scenario.benchmark)
            .collect::<std::collections::BTreeSet<_>>();

        // Assert
        assert_eq!(owners, registered);
    }

    #[test]
    fn should_reject_tier2_to_tier4_optimization_rows_without_required_metadata() {
        // Arrange
        let artifact = r#"{
            "schema_version": "cntryl-stress.v2",
            "summaries": [{
                "benchmark_id": "tier2_subsystem_parser/sql_parser/10k/sql_parser/10k",
                "name": "sql_parser/10k",
                "tier": 2,
                "intent": "batch",
                "primary_metric": "throughput",
                "stats": { "mean": 1000.0, "p50": 1000.0, "p95": 1000.0, "p99": 1000.0 },
                "ns_per_op": { "mean": 1000.0, "p50": 1000.0, "p95": 1000.0, "p99": 1000.0 },
                "diagnostics": [],
                "metadata": {
                    "scenario_id": "perf.sql.parser.10k",
                    "family": "core_read",
                    "benchmark": "tier2_subsystem_parser",
                    "workload": "sql_parser",
                    "fixture_scale": "10k"
                }
            }]
        }"#;

        // Act
        let error = validate_stress_artifact_signal_metadata(artifact).expect_err("metadata error");

        // Assert
        assert!(error.contains("operation_unit"));
        assert!(error.contains("logical_operations_per_iteration"));
    }

    #[test]
    fn should_exclude_informational_rows_from_optimization_metadata_requirements() {
        // Arrange
        let artifact = r#"{
            "schema_version": "cntryl-stress.v2",
            "summaries": [{
                "benchmark_id": "tier4_integration_pgwire/connection_churn/10k/connection_churn/10k",
                "name": "connection_churn/10k",
                "tier": 4,
                "intent": "external",
                "primary_metric": "throughput",
                "stats": { "mean": 1000.0, "p50": 1000.0, "p95": 1000.0, "p99": 1000.0 },
                "ns_per_op": { "mean": 1000.0, "p50": 1000.0, "p95": 1000.0, "p99": 1000.0 },
                "diagnostics": [{ "code": "high_variance", "severity": "warning" }],
                "metadata": {
                    "benchmark": "tier4_integration_pgwire",
                    "workload": "connection_churn",
                    "fixture_scale": "10k",
                    "signal_role": "informational"
                }
            }]
        }"#;

        // Act
        let rows = summarize_stress_artifact_rows(artifact).expect("stress rows");
        let validation = validate_stress_artifact_signal_metadata(artifact);

        // Assert
        assert!(validation.is_ok());
        assert!(!rows[0].is_optimization_signal());
        assert_eq!(rows[0].diagnostic_codes, ["high_variance"]);
    }

    #[test]
    fn should_batch_tier2_planning_samples_with_normalized_operation_counts() {
        // Arrange
        let owners = [
            include_str!("../benches/tier2_subsystem_binder.rs"),
            include_str!("../benches/tier2_subsystem_sql_planning.rs"),
        ];
        let harness = include_str!("../benches/support/stress.rs");
        let contract = include_str!("../docs/performance-contracts.md");
        let invocations = super::workloads::PLANNING_FIXTURE_INVOCATIONS_PER_SAMPLE;
        let mut observed_invocations = 0_usize;

        // Act
        let completed = super::stress::repeat_counted_batch(invocations, || {
            observed_invocations = observed_invocations.saturating_add(1);
            128
        });

        // Assert
        assert_eq!(invocations, 256);
        assert_eq!(observed_invocations, invocations);
        assert_eq!(completed, 32_768);
        for owner in owners {
            assert_eq!(owner.matches("measure_counted_batch").count(), 4);
            assert!(owner.contains("workloads::PLANNING_FIXTURE_INVOCATIONS_PER_SAMPLE"));
        }
        assert!(harness.contains("\"fixture_invocations_per_sample\""));
        assert!(contract.contains("record `fixture_invocations_per_sample=256`"));
    }

    #[test]
    fn should_render_manual_benchmark_report_line() {
        // Arrange
        let benchmark = PerformanceBenchmarkScenario {
            scenario_id: "test.scenario",
            family: "core_read",
            access_family: "relational_index",
            benchmark: "tier3_system_query",
            workload: "mixed_order_scalar_query",
            fixture_scale: "100k",
            fixture_rows: 100_000,
            declared_tier: BenchmarkTier::Tier3,
            timing_mode: BenchmarkTimingMode::Batch,
            operation_unit: "query",
            evidence_role: performance_benchmarks::EvidenceRole::Gate,
            fixture_class: FixtureClass::Representative,
            result_cache_policy: ResultCachePolicy::Disabled,
            client_count: None,
            worker_count: None,
            memory_evidence: "storage.data.reads",
            fallback_evidence: "fallback_reason",
            explain_evidence: "access_path",
            metrics_evidence: "query.latency_ms_total",
        };
        let artifact = r#"{
            "schema_version": "cntryl-stress.v1",
            "summaries": [{
                "benchmark_id": "tier3_system_query/mixed_order_scalar_query/100k",
                "primary_metric": "throughput",
                "stats": {
                    "mean": 500000.0,
                    "p50": 490000.0,
                    "p95": 505000.0,
                    "p99": 510000.0
                },
                "ns_per_op": {
                    "mean": 2000.0,
                    "p50": 2000.0,
                    "p95": 3000.0,
                    "p99": 3000.0
                },
                "metadata": {
                    "scenario_id": "test.scenario",
                    "family": "core_read",
                    "benchmark": "tier3_system_query",
                    "workload": "mixed_order_scalar_query",
                    "fixture_scale": "100k"
                }
            }]
        }"#;

        // Act
        let summary = summarize_stress_artifact(&benchmark, artifact).expect("stress summary");
        let rendered = summary.render_report_line();

        // Assert
        assert!(rendered.contains("test.scenario"));
        assert!(rendered.contains("profile=local-dev-fallback-100k"));
        assert!(rendered.contains("storage=in_memory_midge_fallback"));
        assert!(rendered.contains("workload=mixed_order_scalar_query"));
        assert!(rendered.contains("scale=100k"));
        assert!(rendered.contains("p95=3us"));
        assert!(rendered.contains("throughput=500000.00ops/s"));
        assert!(rendered.contains("fallback_evidence=fallback_reason"));
        assert!(rendered.contains("cache_evidence=plan_cache.entries"));
        assert!(rendered.contains("storage_evidence=storage.data.reads"));
        assert!(rendered.contains("feature_evidence=query.latency_ms_total"));
        assert!(rendered.contains("non_goals=not_sla"));
    }

    #[test]
    fn should_resolve_expected_stress_artifact_paths() {
        // Arrange
        let benchmark =
            benchmark_for_benchmark("tier3_system_query", "mixed_order_scalar_query", "100k")
                .expect("query benchmark");

        // Act
        let path = expected_stress_artifact_path(Path::new("target/stress"), benchmark);

        // Assert
        assert_eq!(
            path,
            PathBuf::from("target/stress/tier3_system_query/latest.json")
        );
    }

    // Merged from tests/performance_column_codec.rs to cut a separate test binary.
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
        assert_eq!(relative_gate_count, 4);
        assert_eq!(forces_plain_baseline, 4);
        assert!(verifies_incompressible_plain);
    }

    #[test]
    fn should_bound_column_codec_pairs_to_evidence_backed_query_windows() {
        // Arrange
        let owner = include_str!("../benches/tier2_subsystem_column_scan.rs");

        // Act
        let compressible_queries = super::workloads::COMPRESSIBLE_COLUMN_CODEC_QUERIES_PER_SAMPLE;
        let fast_pair_queries = super::workloads::FAST_COLUMN_CODEC_QUERIES_PER_SAMPLE;
        let fsst_queries = super::workloads::FSST_COLUMN_CODEC_QUERIES_PER_SAMPLE;
        let records_query_window = owner.contains("\"queries_per_sample\"")
            && owner.contains("scenario.queries_per_sample.to_string()");

        // Assert
        assert_eq!(compressible_queries, 256);
        assert_eq!(fast_pair_queries, 4_096);
        assert_eq!(fsst_queries, 512);
        assert!(records_query_window);
    }

    #[test]
    fn should_measure_compiled_column_plans_without_feedback_persistence() {
        // Arrange
        let owner = include_str!("../benches/tier2_subsystem_column_scan.rs");

        // Act
        let compiles_before_timing = owner.contains("compile_sql_physical_plan_for_diagnostics");
        let executes_physical_plan = owner.contains("execute_physical_plan_for_diagnostics");
        let executes_complete_sql = owner.contains(".execute_sql(");
        let compilation_is_recorded_as_setup = owner
            .find("compile_sql_physical_plan_for_diagnostics")
            .zip(owner.find("let setup_time ="))
            .is_some_and(|(compilation, setup_time)| compilation < setup_time);
        let rejects_feedback_writes = owner.contains("after[\"feedback\"][\"writes\"]")
            && owner.contains("before[\"feedback\"][\"writes\"]");

        // Assert
        assert!(compiles_before_timing);
        assert!(executes_physical_plan);
        assert!(!executes_complete_sql);
        assert!(compilation_is_recorded_as_setup);
        assert!(rejects_feedback_writes);
    }

    #[test]
    fn should_register_paired_alp_query_acceptance_scenarios() {
        // Arrange
        let scenarios = benchmark_scenarios()
            .map(|scenario| (scenario.scenario_id, scenario))
            .collect::<std::collections::BTreeMap<_, _>>();
        let owner = include_str!("../benches/tier2_subsystem_column_scan.rs");
        let fixture = include_str!("../benches/support/workloads/column_codec_context.rs");

        // Act
        let candidate = scenarios.get("perf.column.alp_selective_scan.2k");
        let baseline = scenarios.get("perf.column.alp_plain_scan_baseline.2k");
        let enforces_query_gate =
            owner.contains("require_relative_p95(ALP_CANDIDATE, ALP_BASELINE, 1.05)");
        let verifies_alp_selection =
            fixture.contains("assert_selected_codec") && fixture.contains("\"alp\"");
        let verifies_alp_savings =
            fixture.contains("assert_alp_storage_savings") && fixture.contains("saturating_mul(4)");

        // Assert
        assert_eq!(
            candidate.map(|scenario| (scenario.benchmark, scenario.workload)),
            Some(("tier2_subsystem_column_scan", "alp_selective_scan"))
        );
        assert_eq!(
            baseline.map(|scenario| (scenario.benchmark, scenario.workload)),
            Some(("tier2_subsystem_column_scan", "alp_plain_scan_baseline"))
        );
        assert!(enforces_query_gate);
        assert!(verifies_alp_selection);
        assert!(verifies_alp_savings);
    }

    #[test]
    fn should_register_paired_fsst_query_acceptance_scenarios() {
        // Arrange
        let scenarios = benchmark_scenarios()
            .map(|scenario| (scenario.scenario_id, scenario))
            .collect::<std::collections::BTreeMap<_, _>>();
        let owner = include_str!("../benches/tier2_subsystem_column_scan.rs");
        let fixture = include_str!("../benches/support/workloads/column_codec_context.rs");

        // Act
        let candidate = scenarios.get("perf.column.fsst_selective_scan.2k");
        let baseline = scenarios.get("perf.column.fsst_plain_scan_baseline.2k");
        let enforces_query_gate =
            owner.contains("require_relative_p95(FSST_CANDIDATE, FSST_BASELINE, 1.05)");
        let verifies_fsst_selection =
            fixture.contains("assert_selected_codec") && fixture.contains("\"fsst\"");
        let verifies_fsst_savings = fixture.contains("assert_fsst_storage_savings")
            && fixture.contains("saturating_mul(4)")
            && fixture.contains("saturating_mul(3)");

        // Assert
        assert_eq!(
            candidate.map(|scenario| (scenario.benchmark, scenario.workload)),
            Some(("tier2_subsystem_column_scan", "fsst_selective_scan"))
        );
        assert_eq!(
            baseline.map(|scenario| (scenario.benchmark, scenario.workload)),
            Some(("tier2_subsystem_column_scan", "fsst_plain_scan_baseline"))
        );
        assert!(enforces_query_gate);
        assert!(verifies_fsst_selection);
        assert!(verifies_fsst_savings);
    }

    #[test]
    fn should_document_alp_as_a_promoted_codec_with_retained_evidence() {
        // Arrange
        let roadmap = include_str!("../docs/product-roadmap.md");
        let support = include_str!("../docs/feature-support.md");
        let readiness = include_str!("../docs/production-readiness.md");
        let performance = include_str!("../docs/performance-contracts.md");

        // Act
        let remains_planned = roadmap.contains("- ALP — Planned");
        let duplicated_codec_contract = performance
            .matches("Codec selection is deterministic and automatic.")
            .count();

        // Assert
        assert!(!remains_planned);
        assert!(support.contains("ALP decimal-scaled float blocks"));
        assert!(readiness.contains("should_emit_cross_architecture_stable_alp_bytes"));
        assert!(readiness.contains("perf.column.alp_selective_scan.2k"));
        assert!(readiness.contains("perf.column.alp_plain_scan_baseline.2k"));
        assert_eq!(duplicated_codec_contract, 1);
        assert!(performance.contains(
            "`perf.column.alp_selective_scan.2k` is compared with its forced-plain baseline at a maximum p95 ratio of `1.05`"
        ));
    }

    #[test]
    fn should_document_fsst_as_a_promoted_codec_with_retained_evidence() {
        // Arrange
        let roadmap = include_str!("../docs/product-roadmap.md");
        let support = include_str!("../docs/feature-support.md");
        let readiness = include_str!("../docs/production-readiness.md");
        let performance = include_str!("../docs/performance-contracts.md");

        // Act
        let remains_planned = roadmap.contains("- FSST — Planned");

        // Assert
        assert!(!remains_planned);
        assert!(support.contains("FSST UTF-8 symbol streams"));
        assert!(readiness.contains("should_emit_cross_architecture_stable_fsst_bytes"));
        assert!(readiness.contains("perf.column.fsst_selective_scan.2k"));
        assert!(readiness.contains("perf.column.fsst_plain_scan_baseline.2k"));
        assert!(readiness.contains(
            "Each of the four candidate/baseline pairs owns an independent sampling group"
        ));
        assert!(performance.contains(
            "`perf.column.fsst_selective_scan.2k` is compared with its forced-plain baseline at a maximum p95 ratio of `1.05`"
        ));
    }

    // Merged from tests/performance_column_dml.rs to cut a separate test binary.
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

    // Merged from tests/performance_workstation_profile.rs to cut a separate test binary.
    const PERFORMANCE_WORKSTATION_PROFILE_ID: &str = "workstation-apple-m5-arm64-apfs";

    #[test]
    fn should_register_the_named_apple_m5_evidence_profile() {
        // Arrange
        let profiles = include_str!("../benches/support/performance_benchmark_profiles.rs");

        // Act
        let has_profile = profiles.contains(PERFORMANCE_WORKSTATION_PROFILE_ID);

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
        let selects_disk = harness.contains("\"midge_disk_apfs\" | \"midge_disk_native_linux\"");
        let configures_local_storage =
            harness.contains("std::env::set_var(\"CASSIE_STORAGE_MODE\", \"local\")");

        // Assert
        assert!(selects_disk);
        assert!(configures_local_storage);
    }
}
// Formerly tests/poc_quickstart.rs.
mod poc_quickstart {
    use cassie::app::Cassie;
    use cassie::catalog::canonical_relation_name;
    use cassie::types::Value;

    use super::support_sql as support;
    use support::*;

    fn use_local_storage() {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
    }

    fn execute_poc_setup(cassie: &Cassie, session: &cassie::app::CassieSession) {
        for sql in [
        "CREATE TABLE poc_orders (tenant_id TEXT, status TEXT, created_at INT, title TEXT, total INT)",
        "INSERT INTO poc_orders (tenant_id, status, created_at, title, total) VALUES ('acme', 'open', 1, 'first order', 42), ('acme', 'open', 2, 'second order', 99), ('acme', 'closed', 3, 'closed order', 10), ('other', 'open', 4, 'other tenant', 7)",
        "CREATE INDEX poc_orders_lookup_idx ON poc_orders USING btree (tenant_id, status, created_at)",
    ] {
        cassie.execute_sql(session, sql, vec![]).unwrap();
    }
    }

    #[test]
    fn should_execute_embedded_read_model_poc_flow() {
        // Arrange
        use_local_storage();
        let path = data_dir("embedded");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("poc", None);
        execute_poc_setup(&cassie, &session);
        let health = cassie.health();

        // Act
        let orders = cassie
        .execute_sql(
            &session,
            "SELECT title, total FROM poc_orders WHERE tenant_id = 'acme' AND status = 'open' ORDER BY created_at LIMIT 2",
            vec![],
        )
        .unwrap();
        let totals = cassie
        .execute_sql(
            &session,
            "SELECT status, COUNT(*) AS orders FROM poc_orders WHERE tenant_id = 'acme' GROUP BY status ORDER BY status",
            vec![],
        )
        .unwrap();
        let explain = cassie
        .execute_sql(
            &session,
            "EXPLAIN SELECT title, total FROM poc_orders WHERE tenant_id = 'acme' AND status = 'open' ORDER BY created_at LIMIT 2",
            vec![],
        )
        .unwrap();

        // Assert
        assert_eq!(health["ready"].as_bool(), Some(true));
        assert_eq!(
            orders.rows,
            vec![
                vec![Value::String("first order".to_string()), Value::Int64(42)],
                vec![Value::String("second order".to_string()), Value::Int64(99)],
            ]
        );
        assert_eq!(
            totals.rows,
            vec![
                vec![Value::String("closed".to_string()), Value::Int64(1)],
                vec![Value::String("open".to_string()), Value::Int64(2)],
            ]
        );
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        let expected_index = canonical_relation_name("postgres", "public", "poc_orders_lookup_idx");
        assert!(
            plan.contains(&format!("index={expected_index}")),
            "plan={plan}"
        );

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/benchmark_column_metric_contract.rs.
mod benchmark_column_metric_contract {
    #[test]
    fn should_validate_direct_aggregate_metrics_without_requiring_row_scan_counters() {
        // Arrange
        let source = include_str!("../benches/tier3_system_query.rs");
        let start = source
            .find("fn bench_column_representative")
            .expect("column benchmark function");
        let end = source[start..]
            .find("fn bench_vector_exact_representative")
            .map(|offset| start + offset)
            .expect("next benchmark function");
        let column_benchmark = &source[start..end];

        // Act
        let validates_direct_scan = column_benchmark.contains(
            "assert_metric_increased(&before, &after, \"aggregate_acceleration\", \"scans\")",
        );
        let validates_selected_rows = column_benchmark.contains(
            "assert_metric_increased(&before, &after, \"column_batches\", \"selected_rows\")",
        );
        let requires_projected_scan = [
            "\"scans\"",
            "\"predicate_values\"",
            "\"materialized_values\"",
        ]
        .into_iter()
        .any(|metric| {
            column_benchmark.contains(&format!(
                "assert_metric_increased(&before, &after, \"column_batches\", {metric})"
            ))
        });

        // Assert
        assert!(validates_direct_scan);
        assert!(validates_selected_rows);
        assert!(!requires_projected_scan);
    }
}

// Formerly tests/benchmark_deployment_profile_contract.rs.
mod benchmark_deployment_profile_contract {
    const NATIVE_LINUX_PROFILE_ID: &str = "native-linux-amd64-disk";

    #[test]
    fn should_allow_six_hours_for_the_complete_canonical_benchmark_suite() {
        // Arrange
        let workflow = include_str!("../.github/workflows/bench.yml");
        let complete_suite = workflow
            .split_once("  complete-suite:\n")
            .map(|(_, job)| job)
            .expect("complete-suite benchmark job");

        // Act
        let configured_timeout = complete_suite
            .lines()
            .find(|line| line.trim_start().starts_with("timeout-minutes:"))
            .map(str::trim);
        let required_controls = [
            "if: ${{ github.event_name == 'workflow_dispatch' }}",
            "CASSIE_BENCH_SOAK_DURATION_SECONDS: ${{ inputs.soak_duration_seconds }}",
            "run: cargo bench --bench '*' --locked",
            "benchmark_evidence_contract::should_validate_complete_benchmark_artifact_manifest",
            "path: target/stress/**/latest.json",
            "if-no-files-found: error",
        ];
        let missing_controls = required_controls
            .into_iter()
            .filter(|control| !complete_suite.contains(control))
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(configured_timeout, Some("timeout-minutes: 360"));
        assert!(
            missing_controls.is_empty(),
            "missing complete-suite controls: {missing_controls:?}"
        );
    }

    #[test]
    fn should_register_the_workflow_native_linux_profile() {
        // Arrange
        let workflow = include_str!("../.github/workflows/bench.yml");
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
}

// Formerly tests/benchmark_tier3_join_contract.rs.
mod benchmark_tier3_join_contract {
    #[test]
    fn should_bound_tier3_analytical_queries_beyond_the_product_default_deadline() {
        // Arrange
        let fixture = include_str!("../benches/support/workloads/tier3_query_fixture.rs");

        // Act
        let uses_analytical_timeout = fixture.contains(
            "config.limits.query_timeout_ms = LARGE_ANALYTICAL_BENCHMARK_QUERY_TIMEOUT_MS;",
        );

        // Assert
        assert!(uses_analytical_timeout);
    }

    #[test]
    fn should_index_the_bounded_tier3_join_fixture() {
        // Arrange
        let fixture = include_str!("../benches/support/workloads/tier3_query_fixture.rs");

        // Act
        let creates_join_index = fixture
            .contains("CREATE INDEX bench_join_users_key_idx ON bench_join_users (user_key)");

        // Assert
        assert!(creates_join_index);
    }
}

// Formerly tests/benchmark_trust_contract.rs.
mod benchmark_trust_contract {
    #[test]
    fn should_not_enforce_relative_thresholds_from_untrusted_rows() {
        // Arrange
        let gates = include_str!("../benches/support/stress_relative_gates.rs");

        // Act
        let checks_candidate_trust = gates.contains("!candidate.is_gate()");
        let checks_baseline_trust = gates.contains("!baseline.is_gate()");

        // Assert
        assert!(checks_candidate_trust);
        assert!(checks_baseline_trust);
    }
}

// Shared GitHub Actions topology mirrored from the Fitz repository.
mod workflow_setup_contract {
    use std::fs;

    fn workflow(name: &str) -> String {
        fs::read_to_string(format!(".github/workflows/{name}.yml"))
            .unwrap_or_else(|error| panic!("read {name} workflow: {error}"))
    }

    #[test]
    fn should_match_the_shared_backend_ci_setup() {
        // Arrange
        let backend = workflow("ci-backend");

        // Act
        let shared_controls = [
            "paths-ignore:\n      - \"ui/**\"",
            "group: ${{ github.workflow }}-${{ github.event.pull_request.number || github.ref }}",
            "timeout-minutes: 20",
            "RUSTFLAGS: \"-C link-arg=-fuse-ld=lld\"",
            "github.com/rhysd/actionlint/cmd/actionlint@v1.7.12",
            "0cb0d0c8be5951753580a6f8a527f3e47d293466",
            "cntryl-tools validate-docs --config .cntryl/repository.toml",
            "cntryl-tools validate-benchmarks --config .cntryl/repository.toml",
            "cntryl-tools check-module-sizes --config .cntryl/repository.toml",
            "cargo clippy --locked --workspace --all-targets --all-features",
            "cargo test --locked --workspace",
        ];
        let missing = shared_controls
            .into_iter()
            .filter(|control| !backend.contains(control))
            .collect::<Vec<_>>();

        // Assert
        assert!(missing.is_empty(), "missing backend controls: {missing:?}");
    }

    #[test]
    fn should_match_the_shared_frontend_ci_setup() {
        // Arrange
        let frontend = workflow("ci-frontend");

        // Act
        let shared_controls = [
            "- Dockerfile",
            "node-version: lts/*",
            "run: npm ci",
            "run: npm outdated '@askrjs/*'",
            "docker build --target ui-builder --tag cassie-ui-build .",
            "run: npm run install:browsers",
            "run: npm run test -- --run",
            "run: npm run test:e2e:mock",
            "run: npm run test:e2e",
            "uses: actions/upload-artifact@v7",
        ];
        let missing = shared_controls
            .into_iter()
            .filter(|control| !frontend.contains(control))
            .collect::<Vec<_>>();

        // Assert
        assert!(missing.is_empty(), "missing frontend controls: {missing:?}");
    }

    #[test]
    fn should_match_the_shared_benchmark_workflow_setup() {
        // Arrange
        let bench = workflow("bench");

        // Act
        let shared_controls = [
            "name: Bench",
            "cron: \"0 5 * * *\"",
            "0cb0d0c8be5951753580a6f8a527f3e47d293466",
            "cntryl-tools validate-benchmarks",
            "cargo bench --bench 'tier1*' --locked --quiet",
            "cargo bench --bench 'tier2*' --locked --quiet",
            "cargo bench --bench 'tier3*' --locked --quiet",
            "cargo bench --bench 'tier4*' --locked --quiet",
            "uses: actions/upload-artifact@v7",
        ];
        let missing = shared_controls
            .into_iter()
            .filter(|control| !bench.contains(control))
            .collect::<Vec<_>>();

        // Assert
        assert!(
            missing.is_empty(),
            "missing benchmark controls: {missing:?}"
        );
    }

    #[test]
    fn should_publish_versioned_containers_through_the_shared_release_topology() {
        // Arrange
        let containers = workflow("containers");
        let publish = workflow("publish");
        let version = workflow("version");

        // Act
        let reusable_container_controls = [
            "workflow_call:",
            "release:",
            "value: ${{ jobs.version.outputs.semver }}",
            "name: Validate publish request",
            "Refusing to publish existing repository tag",
            "Refusing to replace immutable SemVer tag",
            "tags+=(-t \"${image}:main\" -t \"${image}:latest\")",
        ];
        let publish_controls = [
            "uses: ./.github/workflows/containers.yml",
            "release: true",
            "needs.containers.outputs.semver",
            "git tag --annotate",
        ];
        let missing_container_controls = reusable_container_controls
            .into_iter()
            .filter(|control| !containers.contains(control))
            .collect::<Vec<_>>();
        let missing_publish_controls = publish_controls
            .into_iter()
            .filter(|control| !publish.contains(control))
            .collect::<Vec<_>>();
        let exposes_obsolete_outputs =
            version.contains("fullSemVer:") || version.contains("branch:");

        // Assert
        assert!(
            missing_container_controls.is_empty(),
            "missing container controls: {missing_container_controls:?}"
        );
        assert!(
            missing_publish_controls.is_empty(),
            "missing publish controls: {missing_publish_controls:?}"
        );
        assert!(!exposes_obsolete_outputs);
    }
}
