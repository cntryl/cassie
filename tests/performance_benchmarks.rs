#[path = "../benches/support/performance_benchmarks.rs"]
pub mod performance_benchmarks;

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
        let scenario =
            benchmark_for_benchmark(owner, workload, "micro").expect("registered Tier 1 scenario");
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
        scenario.fixture_class == FixtureClass::Representative && scenario.fixture_rows == 100_000
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
