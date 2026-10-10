#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use serde_json::{json, Value};

    use crate::performance_benchmarks::{
        benchmark_for_scenario, validate_complete_benchmark_artifacts,
        validate_hnsw_release_evidence, validate_ivfflat_release_evidence, BenchmarkOwnerManifest,
        BenchmarkSuiteManifest,
    };

    const HNSW: &str = "perf.vector.hnsw_persisted.100k";
    const IVFFLAT: &str = "perf.vector.ivfflat_persisted.100k";
    const OWNER: &str = "tier3_system_query";
    const SCENARIOS: [&str; 2] = [HNSW, IVFFLAT];

    struct Outcome {
        case: String,
        result: Result<(), String>,
    }

    fn release_metadata(scenario_id: &str) -> Value {
        let mut metadata = json!({
            "recall_at_k": "0.90",
            "recall_floor": "0.90",
            "exact_top_k": "20",
            "vector_dimensions": "3",
            "distance_metric": "l2",
            "fixture_seed": "0"
        });
        let configuration = if scenario_id == HNSW {
            json!({
                "hnsw_m": "32",
                "hnsw_ef_construction": "256",
                "hnsw_ef_search": "256"
            })
        } else {
            assert_eq!(scenario_id, IVFFLAT);
            json!({
                "filter_selectivity": "unfiltered",
                "ivfflat_lists": "64",
                "ivfflat_probes": "16",
                "ivfflat_training_sample_size": "4096",
                "ivfflat_training_seed": "42"
            })
        };
        metadata.as_object_mut().expect("metadata object").extend(
            configuration
                .as_object()
                .expect("configuration object")
                .clone(),
        );
        metadata
    }

    fn validate_release(scenario_id: &str, metadata: &Value) -> Result<(), String> {
        let scenario = benchmark_for_scenario(scenario_id).expect("registered ANN scenario");
        match scenario_id {
            HNSW => validate_hnsw_release_evidence(scenario, metadata),
            IVFFLAT => validate_ivfflat_release_evidence(scenario, metadata),
            _ => panic!("unexpected test scenario {scenario_id}"),
        }
    }

    fn invalid_outcomes(key: &str, values: &[&str]) -> Vec<Outcome> {
        let mut outcomes = Vec::new();
        for scenario_id in SCENARIOS {
            let valid = release_metadata(scenario_id);
            assert_eq!(validate_release(scenario_id, &valid), Ok(()));
            for value in values {
                let mut metadata = valid.clone();
                metadata[key] = json!(value);
                outcomes.push(Outcome {
                    case: format!("{scenario_id} {key}={value}"),
                    result: validate_release(scenario_id, &metadata),
                });
            }
        }
        outcomes
    }

    fn assert_field_errors(outcomes: &[Outcome], key: &str) {
        for outcome in outcomes {
            eprintln!(
                "ANN numeric boundary {}: {:?}",
                outcome.case, outcome.result
            );
        }
        assert!(
            outcomes
                .iter()
                .all(|outcome| matches!(&outcome.result, Err(error) if error.contains(key))),
            "all malformed cases must fail with the affected field {key}"
        );
    }

    #[test]
    fn should_reject_non_finite_ann_recall_evidence() {
        // Arrange
        let invalid = ["NaN", "inf"];

        // Act
        let outcomes = invalid_outcomes("recall_at_k", &invalid);

        // Assert
        assert_eq!(outcomes.len(), 4);
        assert_field_errors(&outcomes, "recall_at_k");
    }

    #[test]
    fn should_reject_non_finite_ann_recall_floor() {
        // Arrange
        let invalid = ["NaN"];

        // Act
        let outcomes = invalid_outcomes("recall_floor", &invalid);

        // Assert
        assert_eq!(outcomes.len(), 2);
        assert_field_errors(&outcomes, "recall_floor");
    }

    #[test]
    fn should_reject_ann_recall_above_one() {
        // Arrange
        let invalid = ["1.01"];

        // Act
        let outcomes = invalid_outcomes("recall_at_k", &invalid);

        // Assert
        assert_eq!(outcomes.len(), 2);
        assert_field_errors(&outcomes, "recall_at_k");
    }

    #[test]
    fn should_accept_finite_ann_recall_endpoints() {
        // Arrange
        let recalls = [json!("0.90"), json!("1.00"), json!(0.90), json!(1.00)];

        // Act
        let mut outcomes = Vec::new();
        for scenario_id in SCENARIOS {
            for recall in &recalls {
                let mut metadata = release_metadata(scenario_id);
                metadata["recall_at_k"] = recall.clone();
                metadata["recall_floor"] = if recall.is_string() {
                    json!("0.90")
                } else {
                    json!(0.90)
                };
                outcomes.push(Outcome {
                    case: format!("{scenario_id} recall_at_k={recall}"),
                    result: validate_release(scenario_id, &metadata),
                });
            }
        }

        // Assert
        assert_eq!(outcomes.len(), 8);
        for outcome in outcomes {
            eprintln!(
                "ANN numeric boundary {}: {:?}",
                outcome.case, outcome.result
            );
            assert_eq!(outcome.result, Ok(()), "{}", outcome.case);
        }
    }

    fn registered_artifact() -> (BenchmarkSuiteManifest, BTreeMap<String, String>) {
        let scenario = benchmark_for_scenario(HNSW).expect("registered HNSW scenario");
        assert_eq!(scenario.benchmark, OWNER);
        assert_eq!(scenario.declared_tier.number(), 3);
        let mut metadata = release_metadata(HNSW);
        let evidence = json!({
            "scenario_id": HNSW,
            "benchmark": OWNER,
            "workload": scenario.workload,
            "fixture_scale": scenario.fixture_scale,
            "fixture_rows": scenario.fixture_rows.to_string(),
            "fixture_class": format!("{:?}", scenario.fixture_class).to_ascii_lowercase(),
            "operation_unit": scenario.operation_unit,
            "signal_role": scenario.evidence_role.signal_role(),
            "result_cardinality": "20",
            "selected_access_path": "hnsw",
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
            "execution_result_cache_hits": "0",
            "failed_operations": "0"
        });
        metadata
            .as_object_mut()
            .expect("metadata object")
            .extend(evidence.as_object().expect("evidence object").clone());
        let artifact = json!({
            "schema_version": "cntryl-stress.v2",
            "suite": OWNER,
            "run_profile": "default",
            "metadata": {
                "filtered_run": "false",
                "owner_suite_complete": "true",
                "run_id": "synthetic-917-caller"
            },
            "environment": {
                "git_commit": "synthetic-917-source",
                "rustc_version": "synthetic-917-toolchain"
            },
            "summaries": [{"tier": 3, "intent": "batch", "metadata": metadata}],
            "total_elapsed_ns": 2000
        });
        let manifest = BTreeMap::from([(
            OWNER.to_string(),
            BenchmarkOwnerManifest {
                tier: 3,
                scenarios: BTreeSet::from([HNSW.to_string()]),
                result_cache_scenarios: BTreeSet::new(),
            },
        )]);
        (
            manifest,
            BTreeMap::from([(OWNER.to_string(), artifact.to_string())]),
        )
    }

    #[test]
    fn should_reject_nan_recall_in_complete_registered_ann_artifact() {
        // Arrange
        let (manifest, valid_artifacts) = registered_artifact();
        let mut malformed_artifacts = valid_artifacts.clone();
        let mut artifact: Value =
            serde_json::from_str(&malformed_artifacts[OWNER]).expect("synthetic artifact JSON");
        artifact["summaries"][0]["metadata"]["recall_at_k"] = json!("NaN");
        malformed_artifacts.insert(OWNER.to_string(), artifact.to_string());

        // Act
        let valid = validate_complete_benchmark_artifacts(&manifest, &valid_artifacts);
        let malformed = validate_complete_benchmark_artifacts(&manifest, &malformed_artifacts);

        // Assert
        eprintln!("ANN registered synthetic valid: {valid:?}; NaN recall: {malformed:?}");
        assert_eq!(valid, Ok(()), "the unchanged registered artifact must pass");
        assert!(
            matches!(&malformed, Err(error) if error.contains("recall_at_k")),
            "the sole recall mutation must fail with its field name"
        );
    }
}
