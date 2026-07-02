#[path = "../benches/support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "../benches/support/workloads.rs"]
mod workloads;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use performance_benchmarks::{
    benchmark_for_benchmark, benchmark_for_scenario, deployment_profile_for_id,
    deployment_profile_for_scenario, expected_criterion_sample_path, summarize_criterion_sample,
    PerformanceBenchmarkScenario, BENCHMARK_SCENARIOS, BENCHMARK_SCENARIO_PLACEHOLDERS,
    DEPLOYMENT_PROFILES, REQUIRED_WORKLOAD_FAMILIES, SUPPORTED_SCALES,
};

fn metric_names(benchmark: &PerformanceBenchmarkScenario) -> BTreeSet<&'static str> {
    [
        benchmark.memory_evidence,
        benchmark.fallback_evidence,
        benchmark.explain_evidence,
        benchmark.metrics_evidence,
    ]
    .into_iter()
    .collect()
}

fn metric_delta(after: &serde_json::Value, before: &serde_json::Value, path: &[&str]) -> u64 {
    let after_value = path.iter().fold(after, |value, key| &value[*key]);
    let before_value = path.iter().fold(before, |value, key| &value[*key]);
    after_value.as_u64().unwrap() - before_value.as_u64().unwrap()
}

fn join_sql(limit: u32) -> &'static str {
    match limit {
        50 => {
            "SELECT bench_join_users.name, bench_join_orders.total \
             FROM bench_join_users JOIN bench_join_orders \
             ON bench_join_users.user_key = bench_join_orders.order_user_key \
             LIMIT 50"
        }
        500 => {
            "SELECT bench_join_users.name, bench_join_orders.total \
             FROM bench_join_users JOIN bench_join_orders \
             ON bench_join_users.user_key = bench_join_orders.order_user_key \
             LIMIT 500"
        }
        _ => unreachable!("unsupported join limit"),
    }
}

fn render_bounded_join_metric_line(
    workload: &str,
    scale: &str,
    ctx: &workloads::BenchContext,
    sql: &str,
) -> String {
    let before = ctx.cassie.metrics();
    let result = ctx
        .cassie
        .execute_sql(&ctx.session, sql, vec![])
        .expect("bounded join benchmark query");
    let after = ctx.cassie.metrics();

    format!(
        "{workload}/{scale} rows={} scanned_rows={} index_seeks={} build_rows={} probe_rows={} output_rows={} side_reason={}",
        result.rows.len(),
        metric_delta(
            &after,
            &before,
            &["read_paths", "collection_scan_rows"]
        ),
        metric_delta(&after, &before, &["read_paths", "index_seek_scans"]),
        metric_delta(&after, &before, &["joins", "vectorized_build_rows_total"]),
        metric_delta(&after, &before, &["joins", "vectorized_probe_rows_total"]),
        metric_delta(&after, &before, &["joins", "output_rows_total"]),
        after["joins"]["last_bounded_side_selection_reason"]
            .as_str()
            .unwrap_or(""),
    )
}

#[test]
fn should_register_benchmark_for_each_required_fixture() {
    // Arrange
    let mut missing = Vec::new();

    // Act
    for family in REQUIRED_WORKLOAD_FAMILIES {
        for scale in SUPPORTED_SCALES {
            let exists = BENCHMARK_SCENARIOS
                .iter()
                .any(|benchmark| benchmark.family == *family && benchmark.fixture_scale == *scale);
            if !exists {
                missing.push(format!("{family}/{scale}"));
            }
        }
    }

    // Assert
    assert!(
        missing.is_empty(),
        "missing performance benchmarks: {}",
        missing.join(", ")
    );
}

#[test]
fn should_keep_benchmark_scenario_ids_unique() {
    // Arrange
    let mut scenario_ids = BTreeSet::new();

    // Act
    for benchmark in BENCHMARK_SCENARIOS {
        assert!(
            scenario_ids.insert(benchmark.scenario_id),
            "duplicate scenario id {}",
            benchmark.scenario_id
        );
    }

    // Assert
    assert_eq!(scenario_ids.len(), BENCHMARK_SCENARIOS.len());
}

#[test]
fn should_keep_benchmark_benchmark_owners_unique() {
    // Arrange
    let mut benchmark_keys = BTreeSet::new();

    // Act
    for benchmark in BENCHMARK_SCENARIOS {
        assert!(
            benchmark_keys.insert((
                benchmark.benchmark,
                benchmark.workload,
                benchmark.fixture_scale,
            )),
            "duplicate benchmark benchmark owner {}/{}/{}",
            benchmark.benchmark,
            benchmark.workload,
            benchmark.fixture_scale
        );
    }

    // Assert
    assert_eq!(benchmark_keys.len(), BENCHMARK_SCENARIOS.len());
}

#[test]
fn should_register_bounded_join_benchmarks_for_supported_scales() {
    // Arrange
    let workloads = [
        "vectorized_right_indexed_inner_join",
        "vectorized_late_match_inner_join",
        "vectorized_fanout_inner_join",
    ];
    let scales = ["10k", "100k"];

    // Act
    let missing = workloads
        .into_iter()
        .flat_map(|workload| {
            scales
                .into_iter()
                .filter(move |scale| {
                    benchmark_for_benchmark("tier2_subsystem_executor", workload, scale).is_none()
                })
                .map(move |scale| format!("{workload}/{scale}"))
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        missing.is_empty(),
        "missing bounded join benchmarks: {missing:?}"
    );
}

#[test]
fn should_register_mixed_direction_benchmarks_for_supported_scales() {
    // Arrange
    let scales = ["10k", "100k"];

    // Act
    let missing = scales
        .into_iter()
        .filter(|scale| {
            benchmark_for_benchmark("tier3_system_query", "mixed_direction_scalar_query", scale)
                .is_none()
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        missing.is_empty(),
        "missing mixed-direction benchmarks: {missing:?}"
    );
}

#[test]
fn should_lookup_benchmarks_by_scenario() {
    // Arrange
    let scenario_ids = BENCHMARK_SCENARIOS
        .iter()
        .map(|benchmark| benchmark.scenario_id)
        .collect::<Vec<_>>();

    // Act
    let missing = scenario_ids
        .into_iter()
        .filter(|scenario_id| benchmark_for_scenario(scenario_id).is_none())
        .collect::<Vec<_>>();

    // Assert
    assert!(missing.is_empty(), "scenario lookup failed for {missing:?}");
}

#[test]
fn should_lookup_benchmarks_by_benchmark_owner() {
    // Arrange
    let owners = BENCHMARK_SCENARIOS
        .iter()
        .map(|benchmark| {
            (
                benchmark.benchmark,
                benchmark.workload,
                benchmark.fixture_scale,
            )
        })
        .collect::<Vec<_>>();

    // Act
    let missing = owners
        .into_iter()
        .filter(|(benchmark, workload, scale)| {
            benchmark_for_benchmark(benchmark, workload, scale).is_none()
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        missing.is_empty(),
        "benchmark lookup failed for {missing:?}"
    );
}

#[test]
fn should_require_explicit_evidence_labels() {
    // Arrange
    let benchmarks = BENCHMARK_SCENARIOS;

    // Act
    let incomplete = benchmarks
        .iter()
        .filter(|benchmark| metric_names(benchmark).len() != 4)
        .map(|benchmark| benchmark.scenario_id)
        .collect::<Vec<_>>();

    // Assert
    assert!(incomplete.is_empty(), "incomplete evidence: {incomplete:?}");
}

#[test]
fn should_define_complete_deployment_profiles() {
    // Arrange
    let profiles = DEPLOYMENT_PROFILES;

    // Act
    let incomplete = profiles
        .iter()
        .filter(|profile| {
            profile.profile_id.is_empty()
                || profile.host_shape.is_empty()
                || profile.storage_mode.is_empty()
                || profile.data_shape.is_empty()
                || profile.workload_mix.is_empty()
                || profile.fixture_scale.is_empty()
                || profile.benchmark_command.is_empty()
                || profile.metrics_captured.is_empty()
                || profile.known_non_goals.is_empty()
        })
        .map(|profile| profile.profile_id)
        .collect::<Vec<_>>();

    // Assert
    assert!(
        incomplete.is_empty(),
        "incomplete deployment profiles: {incomplete:?}"
    );
}

#[test]
fn should_link_each_benchmark_to_deployment_profile() {
    // Arrange
    let benchmarks = BENCHMARK_SCENARIOS;

    // Act
    let missing = benchmarks
        .iter()
        .filter(|benchmark| deployment_profile_for_scenario(benchmark).is_none())
        .map(|benchmark| benchmark.scenario_id)
        .collect::<Vec<_>>();

    // Assert
    assert!(missing.is_empty(), "missing profiles: {missing:?}");
}

#[test]
fn should_keep_future_fixture_placeholders_out_of_default_scales() {
    // Arrange
    let placeholders = BENCHMARK_SCENARIO_PLACEHOLDERS;

    // Act
    let invalid = placeholders
        .iter()
        .filter(|placeholder| {
            let Some(profile) = deployment_profile_for_scenario(placeholder) else {
                return true;
            };
            SUPPORTED_SCALES.contains(&placeholder.fixture_scale)
                || profile.default_manual
                || !profile.known_non_goals.contains(&"not_default_fixture")
        })
        .map(|placeholder| placeholder.scenario_id)
        .collect::<Vec<_>>();

    // Assert
    assert!(
        invalid.is_empty(),
        "invalid future placeholders: {invalid:?}"
    );
}

#[test]
fn should_parse_criterion_sample_percentiles() {
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
    let sample = r#"{
        "iters": [1, 1, 1],
        "times": [1000.0, 2000.0, 3000.0]
    }"#;

    // Act
    let summary = summarize_criterion_sample(&benchmark, sample).expect("sample summary");

    // Assert
    assert_eq!(summary.scenario_id, "test.scenario");
    assert_eq!(summary.profile_id, "local-dev-fallback-10k");
    assert_eq!(summary.p50_us, 2);
    assert_eq!(summary.p95_us, 3);
    assert_eq!(summary.p99_us, 3);
    assert!((summary.throughput_ops_per_sec - 500_000.0).abs() < f64::EPSILON);
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
    let sample = r#"{
        "iters": [1, 1, 1],
        "times": [1000.0, 2000.0, 3000.0]
    }"#;

    // Act
    let summary = summarize_criterion_sample(&benchmark, sample).expect("sample summary");
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
fn should_resolve_expected_criterion_sample_paths() {
    // Arrange
    let benchmark = benchmark_for_benchmark("tier3_system_query", "simple_sql_query", "10k")
        .expect("query benchmark");

    // Act
    let path = expected_criterion_sample_path(Path::new("target/criterion"), benchmark);

    // Assert
    assert_eq!(
        path,
        PathBuf::from("target/criterion/tier3_system_query/simple_sql_query/10k/new/sample.json")
    );
}

#[test]
fn should_keep_performance_contract_docs_aligned_with_benchmark_ids() {
    // Arrange
    let docs = std::fs::read_to_string("docs/performance-contracts.md")
        .expect("read performance contracts");

    // Act
    let missing = BENCHMARK_SCENARIOS
        .iter()
        .filter(|benchmark| !docs.contains(benchmark.scenario_id))
        .map(|benchmark| benchmark.scenario_id)
        .collect::<Vec<_>>();

    // Assert
    assert!(missing.is_empty(), "docs missing {missing:?}");
}

#[test]
fn should_keep_performance_contract_docs_aligned_with_profile_ids() {
    // Arrange
    let docs = std::fs::read_to_string("docs/performance-contracts.md")
        .expect("read performance contracts");

    // Act
    let missing = DEPLOYMENT_PROFILES
        .iter()
        .filter(|profile| !docs.contains(profile.profile_id))
        .map(|profile| profile.profile_id)
        .collect::<Vec<_>>();

    // Assert
    assert!(missing.is_empty(), "docs missing profiles {missing:?}");
}

#[test]
fn should_lookup_deployment_profiles_by_id() {
    // Arrange
    let profile_ids = DEPLOYMENT_PROFILES
        .iter()
        .map(|profile| profile.profile_id)
        .collect::<Vec<_>>();

    // Act
    let missing = profile_ids
        .into_iter()
        .filter(|profile_id| deployment_profile_for_id(profile_id).is_none())
        .collect::<Vec<_>>();

    // Assert
    assert!(missing.is_empty(), "profile lookup failed for {missing:?}");
}

#[test]
#[ignore = "manual bounded-join evidence; run with --ignored --nocapture"]
fn should_render_bounded_join_metric_evidence_for_manual_review() {
    // Arrange
    let runtime = workloads::runtime();
    let mut report = Vec::new();

    // Act
    for (scale, rows) in [("10k", 10_000), ("100k", 100_000)] {
        let right_indexed = runtime
            .block_on(workloads::vectorized_right_indexed_join_context(
                &format!("metric-right-indexed-{scale}"),
                rows,
            ))
            .expect("right-indexed bounded join context");
        report.push(render_bounded_join_metric_line(
            "vectorized_right_indexed_inner_join",
            scale,
            &right_indexed,
            join_sql(50),
        ));

        let late_match = runtime
            .block_on(workloads::vectorized_late_match_join_context(
                &format!("metric-late-match-{scale}"),
                rows,
            ))
            .expect("late-match bounded join context");
        report.push(render_bounded_join_metric_line(
            "vectorized_late_match_inner_join",
            scale,
            &late_match,
            join_sql(50),
        ));

        let fanout = runtime
            .block_on(workloads::vectorized_fanout_join_context(
                &format!("metric-fanout-{scale}"),
                rows,
            ))
            .expect("fanout bounded join context");
        report.push(render_bounded_join_metric_line(
            "vectorized_fanout_inner_join",
            scale,
            &fanout,
            join_sql(500),
        ));
    }

    // Assert
    assert_eq!(report.len(), 6);
    eprintln!("{}", report.join("\n"));
}

#[test]
#[ignore = "requires Criterion output from cargo bench; run with --nocapture for the report"]
fn should_render_generated_criterion_output_for_manual_review() {
    // Arrange
    let criterion_root = Path::new("target/criterion");
    let mut report = Vec::new();

    // Act
    for benchmark in BENCHMARK_SCENARIOS {
        let path = expected_criterion_sample_path(criterion_root, benchmark);
        let sample = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let summary =
            summarize_criterion_sample(benchmark, &sample).expect("summarize criterion sample");
        report.push(summary.render_report_line());
    }

    // Assert
    assert_eq!(report.len(), BENCHMARK_SCENARIOS.len());
    eprintln!("{}", report.join("\n"));
}
