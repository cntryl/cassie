#![allow(dead_code)]

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PerformanceBenchmarkScenario {
    pub scenario_id: &'static str,
    pub family: &'static str,
    pub benchmark: &'static str,
    pub workload: &'static str,
    pub fixture_scale: &'static str,
    pub memory_evidence: &'static str,
    pub fallback_evidence: &'static str,
    pub explain_evidence: &'static str,
    pub metrics_evidence: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeploymentProfile {
    pub profile_id: &'static str,
    pub host_shape: &'static str,
    pub storage_mode: &'static str,
    pub data_shape: &'static str,
    pub workload_mix: &'static str,
    pub fixture_scale: &'static str,
    pub benchmark_command: &'static str,
    pub cache_evidence: &'static str,
    pub metrics_captured: &'static [&'static str],
    pub known_non_goals: &'static [&'static str],
    pub default_manual: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BenchmarkSampleSummary {
    pub profile_id: &'static str,
    pub scenario_id: &'static str,
    pub benchmark: &'static str,
    pub workload: &'static str,
    pub fixture_scale: &'static str,
    pub storage_mode: &'static str,
    pub storage_evidence: &'static str,
    pub fallback_evidence: &'static str,
    pub cache_evidence: &'static str,
    pub feature_evidence: &'static str,
    pub known_non_goals: &'static [&'static str],
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub throughput_ops_per_sec: f64,
}

#[derive(Debug, serde::Deserialize)]
struct CriterionSample {
    iters: Vec<f64>,
    times: Vec<f64>,
}

pub const SUPPORTED_SCALES: &[&str] = &["10k", "100k"];

const STANDARD_METRICS_CAPTURED: &[&str] = &[
    "p50_us",
    "p95_us",
    "p99_us",
    "throughput_ops_per_sec",
    "fallback_counters",
    "cache_occupancy",
    "storage_family_operations",
    "feature_family_metrics",
];

const LOCAL_PROFILE_NON_GOALS: &[&str] = &[
    "not_sla",
    "not_ci_gate",
    "not_production_ready_promotion",
    "not_disk_sync_unless_bench_midge_disk",
];

const FUTURE_PROFILE_NON_GOALS: &[&str] = &[
    "not_sla",
    "not_ci_gate",
    "not_production_ready_promotion",
    "not_default_fixture",
    "not_required_by_current_benchmarks",
];

pub const DEPLOYMENT_PROFILES: &[DeploymentProfile] = &[
    deployment_profile(
        "local-dev-fallback-10k",
        "local developer workstation",
        "in_memory_midge_fallback",
        "deterministic generated read-model fixture",
        "single benchmark owner workload",
        "10k",
        "cargo bench --locked --bench <owner-benchmark>",
        "plan_cache.entries",
        STANDARD_METRICS_CAPTURED,
        LOCAL_PROFILE_NON_GOALS,
        true,
    ),
    deployment_profile(
        "local-dev-fallback-100k",
        "local developer workstation",
        "in_memory_midge_fallback",
        "deterministic generated read-model fixture",
        "single benchmark owner workload",
        "100k",
        "cargo bench --locked --bench <owner-benchmark>",
        "plan_cache.entries",
        STANDARD_METRICS_CAPTURED,
        LOCAL_PROFILE_NON_GOALS,
        true,
    ),
    deployment_profile(
        "future-1m-placeholder",
        "declared deployment profile",
        "profile-defined",
        "future generated read-model fixture",
        "single benchmark owner workload",
        "1M",
        "cargo bench --locked --bench <owner-benchmark> --no-run",
        "plan_cache.entries",
        STANDARD_METRICS_CAPTURED,
        FUTURE_PROFILE_NON_GOALS,
        false,
    ),
];

pub const REQUIRED_WORKLOAD_FAMILIES: &[&str] = &[
    "core_read",
    "replay",
    "rebuild",
    "verification",
    "search",
    "vector",
    "hybrid",
    "time_series",
    "pgwire",
    "http",
];

pub const BENCHMARK_SCENARIOS: &[PerformanceBenchmarkScenario] = &[
    benchmark(
        "perf.core_read.simple.10k",
        "core_read",
        "tier3_system_query",
        "simple_sql_query",
        "10k",
        "storage.data.reads",
        "fallback_reason",
        "access_path=point_lookup",
        "query.latency_ms_total",
    ),
    benchmark(
        "perf.core_read.simple.100k",
        "core_read",
        "tier3_system_query",
        "simple_sql_query",
        "100k",
        "storage.data.reads",
        "fallback_reason",
        "access_path=point_lookup",
        "query.latency_ms_total",
    ),
    benchmark(
        "perf.replay.lag_catchup.10k",
        "replay",
        "tier2_subsystem_ingest",
        "projection_lag_catchup",
        "10k",
        "projections.write_batch_flushes",
        "projections.replay_duplicates_skipped",
        "replay_checkpoint",
        "projections.replay_events_applied",
    ),
    benchmark(
        "perf.replay.lag_catchup.100k",
        "replay",
        "tier2_subsystem_ingest",
        "projection_lag_catchup",
        "100k",
        "projections.write_batch_flushes",
        "projections.replay_duplicates_skipped",
        "replay_checkpoint",
        "projections.replay_events_applied",
    ),
    benchmark(
        "perf.rebuild.refresh.10k",
        "rebuild",
        "tier3_system_rebuild",
        "projection_refresh",
        "10k",
        "projections.write_rebuild_target_puts",
        "rebuild_fallback",
        "materialized_projection_refresh",
        "projections.refreshes",
    ),
    benchmark(
        "perf.rebuild.refresh.100k",
        "rebuild",
        "tier3_system_rebuild",
        "projection_refresh",
        "100k",
        "projections.write_rebuild_target_puts",
        "rebuild_fallback",
        "materialized_projection_refresh",
        "projections.refreshes",
    ),
    benchmark(
        "perf.time_series.window_scan.10k",
        "time_series",
        "tier3_system_query",
        "time_series_window_scan",
        "10k",
        "time_series.buckets_scanned",
        "time_series.fallback_reason",
        "time_series=bucket_width:1 hour",
        "time_series.scans",
    ),
    benchmark(
        "perf.time_series.window_scan.100k",
        "time_series",
        "tier3_system_query",
        "time_series_window_scan",
        "100k",
        "time_series.buckets_scanned",
        "time_series.fallback_reason",
        "time_series=bucket_width:1 hour",
        "time_series.scans",
    ),
    benchmark(
        "perf.time_series.retention.10k",
        "time_series",
        "tier3_system_rebuild",
        "time_series_retention_enforcement",
        "10k",
        "retention.skipped_rows",
        "retention.errors",
        "ENFORCE RETENTION",
        "retention.deleted_rows",
    ),
    benchmark(
        "perf.time_series.retention.100k",
        "time_series",
        "tier3_system_rebuild",
        "time_series_retention_enforcement",
        "100k",
        "retention.skipped_rows",
        "retention.errors",
        "ENFORCE RETENTION",
        "retention.deleted_rows",
    ),
    benchmark(
        "perf.time_series.rollup_refresh.10k",
        "time_series",
        "tier3_system_rebuild",
        "time_series_rollup_refresh",
        "10k",
        "rollups.refreshes",
        "rollups.stale_fallbacks",
        "REFRESH ROLLUP",
        "rollups.rewrite_hits",
    ),
    benchmark(
        "perf.time_series.rollup_refresh.100k",
        "time_series",
        "tier3_system_rebuild",
        "time_series_rollup_refresh",
        "100k",
        "rollups.refreshes",
        "rollups.stale_fallbacks",
        "REFRESH ROLLUP",
        "rollups.rewrite_hits",
    ),
    benchmark(
        "perf.verification.full.10k",
        "verification",
        "tier3_system_rebuild",
        "projection_verify",
        "10k",
        "projection_hash_rows",
        "verification_mismatch_count",
        "VERIFY PROJECTION",
        "projections.verifications",
    ),
    benchmark(
        "perf.verification.full.100k",
        "verification",
        "tier3_system_rebuild",
        "projection_verify",
        "100k",
        "projection_hash_rows",
        "verification_mismatch_count",
        "VERIFY PROJECTION",
        "projections.verifications",
    ),
    benchmark(
        "perf.search.fulltext.10k",
        "search",
        "tier2_subsystem_search",
        "full_text_executor",
        "10k",
        "search.candidate_count_total",
        "search_fallback",
        "access_path=fulltext",
        "search.latency_ms_total",
    ),
    benchmark(
        "perf.search.fulltext.100k",
        "search",
        "tier2_subsystem_search",
        "full_text_executor",
        "100k",
        "search.candidate_count_total",
        "search_fallback",
        "access_path=fulltext",
        "search.latency_ms_total",
    ),
    benchmark(
        "perf.vector.executor.10k",
        "vector",
        "tier2_subsystem_vector",
        "vector_executor",
        "10k",
        "vector.candidate_count_total",
        "vector.normalized_fallback_count_total",
        "access_path=vector",
        "vector.latency_ms_total",
    ),
    benchmark(
        "perf.vector.executor.100k",
        "vector",
        "tier2_subsystem_vector",
        "vector_executor",
        "100k",
        "vector.candidate_count_total",
        "vector.normalized_fallback_count_total",
        "access_path=vector",
        "vector.latency_ms_total",
    ),
    benchmark(
        "perf.hybrid.executor.10k",
        "hybrid",
        "tier2_subsystem_hybrid",
        "hybrid_executor",
        "10k",
        "hybrid.candidate_count_total",
        "hybrid.prefilter_fallback_count_total",
        "mixed_execution",
        "hybrid.latency_ms_total",
    ),
    benchmark(
        "perf.hybrid.executor.100k",
        "hybrid",
        "tier2_subsystem_hybrid",
        "hybrid_executor",
        "100k",
        "hybrid.candidate_count_total",
        "hybrid.prefilter_fallback_count_total",
        "mixed_execution",
        "hybrid.latency_ms_total",
    ),
    benchmark(
        "perf.pgwire.simple_query.10k",
        "pgwire",
        "tier4_integration_pgwire",
        "pgwire_simple_query",
        "10k",
        "pgwire.blocking_elapsed_ms_total",
        "pgwire.protocol_errors_total",
        "pgwire_simple_query",
        "pgwire.simple_queries_total",
    ),
    benchmark(
        "perf.pgwire.simple_query.100k",
        "pgwire",
        "tier4_integration_pgwire",
        "pgwire_simple_query",
        "100k",
        "pgwire.blocking_elapsed_ms_total",
        "pgwire.protocol_errors_total",
        "pgwire_simple_query",
        "pgwire.simple_queries_total",
    ),
    benchmark(
        "perf.http.document_create_get.10k",
        "http",
        "tier4_integration_http",
        "http_document_create_get",
        "10k",
        "storage.data.writes",
        "rest.blocking_error_total",
        "documents::create/get",
        "rest.requests_total",
    ),
    benchmark(
        "perf.http.document_create_get.100k",
        "http",
        "tier4_integration_http",
        "http_document_create_get",
        "100k",
        "storage.data.writes",
        "rest.blocking_error_total",
        "documents::create/get",
        "rest.requests_total",
    ),
];

pub const BENCHMARK_SCENARIO_PLACEHOLDERS: &[PerformanceBenchmarkScenario] = &[
    benchmark(
        "perf.core_read.simple.1m",
        "core_read",
        "tier3_system_query",
        "simple_sql_query",
        "1M",
        "storage.data.reads",
        "fallback_reason",
        "access_path=point_lookup",
        "query.latency_ms_total",
    ),
    benchmark(
        "perf.replay.lag_catchup.1m",
        "replay",
        "tier2_subsystem_ingest",
        "projection_lag_catchup",
        "1M",
        "projections.write_batch_flushes",
        "projections.replay_duplicates_skipped",
        "replay_checkpoint",
        "projections.replay_events_applied",
    ),
    benchmark(
        "perf.rebuild.refresh.1m",
        "rebuild",
        "tier3_system_rebuild",
        "projection_refresh",
        "1M",
        "projections.write_rebuild_target_puts",
        "rebuild_fallback",
        "materialized_projection_refresh",
        "projections.refreshes",
    ),
    benchmark(
        "perf.verification.full.1m",
        "verification",
        "tier3_system_rebuild",
        "projection_verify",
        "1M",
        "projection_hash_rows",
        "verification_mismatch_count",
        "VERIFY PROJECTION",
        "projections.verifications",
    ),
    benchmark(
        "perf.search.fulltext.1m",
        "search",
        "tier2_subsystem_search",
        "full_text_executor",
        "1M",
        "search.candidate_count_total",
        "search_fallback",
        "access_path=fulltext",
        "search.latency_ms_total",
    ),
    benchmark(
        "perf.vector.executor.1m",
        "vector",
        "tier2_subsystem_vector",
        "vector_executor",
        "1M",
        "vector.candidate_count_total",
        "vector.normalized_fallback_count_total",
        "access_path=vector",
        "vector.latency_ms_total",
    ),
    benchmark(
        "perf.hybrid.executor.1m",
        "hybrid",
        "tier2_subsystem_hybrid",
        "hybrid_executor",
        "1M",
        "hybrid.candidate_count_total",
        "hybrid.prefilter_fallback_count_total",
        "mixed_execution",
        "hybrid.latency_ms_total",
    ),
    benchmark(
        "perf.time_series.window_scan.1m",
        "time_series",
        "tier3_system_query",
        "time_series_window_scan",
        "1M",
        "time_series.buckets_scanned",
        "time_series.fallback_reason",
        "time_series=bucket_width:1 hour",
        "time_series.scans",
    ),
    benchmark(
        "perf.pgwire.simple_query.1m",
        "pgwire",
        "tier4_integration_pgwire",
        "pgwire_simple_query",
        "1M",
        "pgwire.blocking_elapsed_ms_total",
        "pgwire.protocol_errors_total",
        "pgwire_simple_query",
        "pgwire.simple_queries_total",
    ),
    benchmark(
        "perf.http.document_create_get.1m",
        "http",
        "tier4_integration_http",
        "http_document_create_get",
        "1M",
        "storage.data.writes",
        "rest.blocking_error_total",
        "documents::create/get",
        "rest.requests_total",
    ),
];

#[allow(clippy::too_many_arguments)]
const fn deployment_profile(
    profile_id: &'static str,
    host_shape: &'static str,
    storage_mode: &'static str,
    data_shape: &'static str,
    workload_mix: &'static str,
    fixture_scale: &'static str,
    benchmark_command: &'static str,
    cache_evidence: &'static str,
    metrics_captured: &'static [&'static str],
    known_non_goals: &'static [&'static str],
    default_manual: bool,
) -> DeploymentProfile {
    DeploymentProfile {
        profile_id,
        host_shape,
        storage_mode,
        data_shape,
        workload_mix,
        fixture_scale,
        benchmark_command,
        cache_evidence,
        metrics_captured,
        known_non_goals,
        default_manual,
    }
}

#[allow(clippy::too_many_arguments)]
const fn benchmark(
    scenario_id: &'static str,
    family: &'static str,
    benchmark: &'static str,
    workload: &'static str,
    fixture_scale: &'static str,
    memory_evidence: &'static str,
    fallback_evidence: &'static str,
    explain_evidence: &'static str,
    metrics_evidence: &'static str,
) -> PerformanceBenchmarkScenario {
    PerformanceBenchmarkScenario {
        scenario_id,
        family,
        benchmark,
        workload,
        fixture_scale,
        memory_evidence,
        fallback_evidence,
        explain_evidence,
        metrics_evidence,
    }
}

pub fn benchmark_for_scenario(scenario_id: &str) -> Option<&'static PerformanceBenchmarkScenario> {
    BENCHMARK_SCENARIOS
        .iter()
        .find(|benchmark| benchmark.scenario_id == scenario_id)
}

pub fn benchmark_for_benchmark(
    benchmark_name: &str,
    workload: &str,
    fixture_scale: &str,
) -> Option<&'static PerformanceBenchmarkScenario> {
    BENCHMARK_SCENARIOS.iter().find(|scenario| {
        scenario.benchmark == benchmark_name
            && scenario.workload == workload
            && scenario.fixture_scale == fixture_scale
    })
}

pub fn expect_benchmark(
    benchmark: &str,
    workload: &str,
    fixture_scale: &str,
) -> &'static PerformanceBenchmarkScenario {
    benchmark_for_benchmark(benchmark, workload, fixture_scale).unwrap_or_else(|| {
        panic!("missing performance benchmark for {benchmark}/{workload}/{fixture_scale}")
    })
}

pub fn deployment_profile_for_id(profile_id: &str) -> Option<&'static DeploymentProfile> {
    DEPLOYMENT_PROFILES
        .iter()
        .find(|profile| profile.profile_id == profile_id)
}

pub fn deployment_profile_for_scenario(
    benchmark: &PerformanceBenchmarkScenario,
) -> Option<&'static DeploymentProfile> {
    DEPLOYMENT_PROFILES
        .iter()
        .find(|profile| profile.fixture_scale == benchmark.fixture_scale)
}

pub fn expected_criterion_sample_path(
    criterion_root: &Path,
    benchmark: &PerformanceBenchmarkScenario,
) -> PathBuf {
    criterion_root
        .join(benchmark.benchmark)
        .join(benchmark.workload)
        .join(benchmark.fixture_scale)
        .join("new")
        .join("sample.json")
}

pub fn summarize_criterion_sample(
    benchmark: &PerformanceBenchmarkScenario,
    sample_json: &str,
) -> Result<BenchmarkSampleSummary, String> {
    let profile = deployment_profile_for_scenario(benchmark).ok_or_else(|| {
        format!(
            "missing deployment profile for scenario {} scale {}",
            benchmark.scenario_id, benchmark.fixture_scale
        )
    })?;
    let sample: CriterionSample =
        serde_json::from_str(sample_json).map_err(|error| error.to_string())?;
    if sample.iters.is_empty() || sample.times.is_empty() {
        return Err("criterion sample has no measurements".to_string());
    }
    if sample.iters.len() != sample.times.len() {
        return Err(format!(
            "criterion sample length mismatch: {} iters, {} times",
            sample.iters.len(),
            sample.times.len()
        ));
    }

    let mut per_iteration_us = sample
        .iters
        .iter()
        .zip(sample.times.iter())
        .map(|(iters, nanos)| {
            if *iters <= 0.0 {
                return Err("criterion sample iteration count must be positive".to_string());
            }
            let per_iteration_ns = nanos / iters;
            Ok((per_iteration_ns / 1_000.0).ceil() as u64)
        })
        .collect::<Result<Vec<_>, _>>()?;
    per_iteration_us.sort_unstable();

    let total_iters = sample.iters.iter().sum::<f64>();
    let total_nanos = sample.times.iter().sum::<f64>();
    if total_nanos <= 0.0 {
        return Err("criterion sample total time must be positive".to_string());
    }

    let p50_us = percentile_us(&per_iteration_us, 0.50);
    let p95_us = percentile_us(&per_iteration_us, 0.95);
    let p99_us = percentile_us(&per_iteration_us, 0.99);
    let throughput_ops_per_sec = total_iters * 1_000_000_000.0 / total_nanos;

    Ok(BenchmarkSampleSummary {
        profile_id: profile.profile_id,
        scenario_id: benchmark.scenario_id,
        benchmark: benchmark.benchmark,
        workload: benchmark.workload,
        fixture_scale: benchmark.fixture_scale,
        storage_mode: profile.storage_mode,
        storage_evidence: benchmark.memory_evidence,
        fallback_evidence: benchmark.fallback_evidence,
        cache_evidence: profile.cache_evidence,
        feature_evidence: benchmark.metrics_evidence,
        known_non_goals: profile.known_non_goals,
        p50_us,
        p95_us,
        p99_us,
        throughput_ops_per_sec,
    })
}

fn percentile_us(sorted: &[u64], percentile: f64) -> u64 {
    let index = ((sorted.len().saturating_sub(1)) as f64 * percentile).ceil() as usize;
    sorted[index.min(sorted.len().saturating_sub(1))]
}

impl BenchmarkSampleSummary {
    pub fn render_report_line(&self) -> String {
        format!(
            "{} profile={} benchmark={} workload={} scale={} storage={} p50={}us p95={}us p99={}us throughput={:.2}ops/s fallback_evidence={} cache_evidence={} storage_evidence={} feature_evidence={} non_goals={}",
            self.scenario_id,
            self.profile_id,
            self.benchmark,
            self.workload,
            self.fixture_scale,
            self.storage_mode,
            self.p50_us,
            self.p95_us,
            self.p99_us,
            self.throughput_ops_per_sec,
            self.fallback_evidence,
            self.cache_evidence,
            self.storage_evidence,
            self.feature_evidence,
            self.known_non_goals.join("|"),
        )
    }
}
