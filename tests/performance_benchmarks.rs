#[path = "../benches/support/performance_benchmarks.rs"]
mod performance_benchmarks;

use std::path::{Path, PathBuf};

use performance_benchmarks::{
    benchmark_for_benchmark, expected_stress_artifact_path, summarize_stress_artifact,
    PerformanceBenchmarkScenario,
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
