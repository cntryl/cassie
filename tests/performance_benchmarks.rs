#[path = "../benches/support/performance_benchmarks.rs"]
pub mod performance_benchmarks;

use std::path::{Path, PathBuf};

use performance_benchmarks::{
    benchmark_for_benchmark, benchmark_scenarios, deployment_profile_for_id,
    expected_stress_artifact_path, summarize_stress_artifact, summarize_stress_artifact_rows,
    validate_stress_artifact_signal_metadata, PerformanceBenchmarkScenario,
    REQUIRED_WORKLOAD_FAMILIES,
};

#[test]
fn should_parse_stress_artifact_percentiles() {
    // Arrange
    let benchmark = PerformanceBenchmarkScenario {
        scenario_id: "test.scenario",
        family: "core_read",
        benchmark: "tier3_system_query",
        workload: "simple_sql_query",
        fixture_scale: "10k",
        memory_evidence: "storage.data.reads",
        fallback_evidence: "fallback_reason",
        explain_evidence: "access_path",
        metrics_evidence: "query.latency_ms_total",
    };
    let artifact = r#"{
        "schema_version": "cntryl-stress.v1",
        "summaries": [{
            "benchmark_id": "tier3_system_query/simple_sql_query/10k",
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
                "workload": "simple_sql_query",
                "fixture_scale": "10k"
            }
        }]
    }"#;

    // Act
    let summary = summarize_stress_artifact(&benchmark, artifact).expect("stress summary");

    // Assert
    assert_eq!(summary.scenario_id, "test.scenario");
    assert_eq!(summary.profile_id, "local-dev-fallback-10k");
    assert_eq!(summary.p50_us, 2);
    assert_eq!(summary.p95_us, 3);
    assert_eq!(summary.p99_us, 3);
    assert!((summary.throughput_ops_per_sec - 500_000.0).abs() < f64::EPSILON);
}

#[test]
fn should_reject_wrong_stress_schema_version() {
    // Arrange
    let benchmark = benchmark_for_benchmark("tier3_system_query", "simple_sql_query", "10k")
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
        benchmark: "tier3_system_query",
        workload: "simple_sql_query",
        fixture_scale: "10k",
        memory_evidence: "storage.data.reads",
        fallback_evidence: "fallback_reason",
        explain_evidence: "access_path",
        metrics_evidence: "query.latency_ms_total",
    };
    let artifact = r#"{
        "schema_version": "cntryl-stress.v2",
        "summaries": [{
            "benchmark_id": "tier3_system_query/simple_sql_query/10k/simple_sql_query/10k",
            "name": "simple_sql_query/10k",
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
                "workload": "simple_sql_query",
                "fixture_scale": "10k",
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
        benchmark: "tier3_system_query",
        workload: "simple_sql_query",
        fixture_scale: "10k",
        memory_evidence: "storage.data.reads",
        fallback_evidence: "fallback_reason",
        explain_evidence: "access_path",
        metrics_evidence: "query.latency_ms_total",
    };
    let artifact = r#"{
        "schema_version": "cntryl-stress.v1",
        "summaries": [{
            "benchmark_id": "tier3_system_query/simple_sql_query/10k",
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
                "workload": "simple_sql_query",
                "fixture_scale": "10k"
            }
        }]
    }"#;

    // Act
    let summary = summarize_stress_artifact(&benchmark, artifact).expect("stress summary");
    let rendered = summary.render_report_line();

    // Assert
    assert!(rendered.contains("test.scenario"));
    assert!(rendered.contains("profile=local-dev-fallback-10k"));
    assert!(rendered.contains("storage=in_memory_midge_fallback"));
    assert!(rendered.contains("workload=simple_sql_query"));
    assert!(rendered.contains("scale=10k"));
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
    let benchmark = benchmark_for_benchmark("tier3_system_query", "simple_sql_query", "10k")
        .expect("query benchmark");

    // Act
    let path = expected_stress_artifact_path(Path::new("target/stress"), benchmark);

    // Assert
    assert_eq!(
        path,
        PathBuf::from("target/stress/tier3_system_query/latest.json")
    );
}
