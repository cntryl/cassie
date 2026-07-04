use std::path::{Path, PathBuf};

use super::{
    deployment_profile_for_scenario, BenchmarkSampleSummary, PerformanceBenchmarkScenario,
};

const STRESS_SCHEMA_VERSION: &str = "cntryl-stress.v1";

pub fn expected_stress_artifact_path(
    stress_root: &Path,
    benchmark: &PerformanceBenchmarkScenario,
) -> PathBuf {
    stress_root.join(benchmark.benchmark).join("latest.json")
}

pub fn summarize_stress_artifact(
    benchmark: &PerformanceBenchmarkScenario,
    artifact_json: &str,
) -> Result<BenchmarkSampleSummary, String> {
    let profile = deployment_profile_for_scenario(benchmark).ok_or_else(|| {
        format!(
            "missing deployment profile for scenario {} scale {}",
            benchmark.scenario_id, benchmark.fixture_scale
        )
    })?;
    let artifact: serde_json::Value =
        serde_json::from_str(artifact_json).map_err(|error| error.to_string())?;
    let schema_version = artifact["schema_version"].as_str().unwrap_or_default();
    if schema_version != STRESS_SCHEMA_VERSION {
        return Err(format!("unsupported schema_version '{schema_version}'"));
    }

    let summary = artifact["summaries"]
        .as_array()
        .and_then(|summaries| {
            summaries
                .iter()
                .find(|summary| summary_matches_benchmark(summary, benchmark))
        })
        .ok_or_else(|| {
            format!(
                "missing stress summary for {}/{}/{}",
                benchmark.benchmark, benchmark.workload, benchmark.fixture_scale
            )
        })?;

    let ns_per_op = summary
        .get("ns_per_op")
        .or_else(|| summary.get("stats"))
        .ok_or_else(|| "stress summary has no timing statistics".to_string())?;
    let p50_us = stat_microseconds(ns_per_op, "p50")?;
    let p95_us = stat_microseconds(ns_per_op, "p95")?;
    let p99_us = stat_microseconds(ns_per_op, "p99")?;
    let throughput_ops_per_sec = throughput(summary, ns_per_op)?;

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

fn ceil_microseconds(value: f64) -> Result<u64, String> {
    if !value.is_finite() || value < 0.0 {
        return Err("stress sample duration must be finite and non-negative".to_string());
    }

    format!("{:.0}", value.ceil())
        .parse::<u64>()
        .map_err(|error| error.to_string())
}

fn summary_matches_benchmark(
    summary: &serde_json::Value,
    benchmark: &PerformanceBenchmarkScenario,
) -> bool {
    let metadata = &summary["metadata"];
    metadata["benchmark"].as_str() == Some(benchmark.benchmark)
        && metadata["workload"].as_str() == Some(benchmark.workload)
        && metadata["fixture_scale"].as_str() == Some(benchmark.fixture_scale)
}

fn stat_microseconds(stats: &serde_json::Value, key: &str) -> Result<u64, String> {
    let nanos = stats[key]
        .as_f64()
        .ok_or_else(|| format!("stress summary missing {key}"))?;
    ceil_microseconds(nanos / 1_000.0)
}

fn throughput(summary: &serde_json::Value, ns_per_op: &serde_json::Value) -> Result<f64, String> {
    if summary["primary_metric"].as_str() == Some("throughput") {
        let stats = summary
            .get("stats")
            .ok_or_else(|| "stress summary has no throughput statistics".to_string())?;
        return stats["mean"]
            .as_f64()
            .filter(|value| value.is_finite() && *value >= 0.0)
            .ok_or_else(|| {
                "stress summary throughput must be finite and non-negative".to_string()
            });
    }

    let mean_ns = ns_per_op["mean"]
        .as_f64()
        .ok_or_else(|| "stress summary missing ns_per_op mean".to_string())?;
    if mean_ns <= 0.0 || !mean_ns.is_finite() {
        return Err("stress summary ns_per_op mean must be positive".to_string());
    }
    Ok(1_000_000_000.0 / mean_ns)
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
