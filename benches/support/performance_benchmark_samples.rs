use std::path::{Path, PathBuf};

use super::{
    deployment_profile_for_scenario, BenchmarkSampleSummary, PerformanceBenchmarkScenario,
};

#[derive(Debug, serde::Deserialize)]
struct CriterionSample {
    iters: Vec<f64>,
    times: Vec<f64>,
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
            ceil_microseconds(per_iteration_ns / 1_000.0)
        })
        .collect::<Result<Vec<_>, _>>()?;
    per_iteration_us.sort_unstable();

    let total_iters = sample.iters.iter().sum::<f64>();
    let total_nanos = sample.times.iter().sum::<f64>();
    if total_nanos <= 0.0 {
        return Err("criterion sample total time must be positive".to_string());
    }

    let p50_us = percentile_us(&per_iteration_us, 50, 100);
    let p95_us = percentile_us(&per_iteration_us, 95, 100);
    let p99_us = percentile_us(&per_iteration_us, 99, 100);
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

fn ceil_microseconds(value: f64) -> Result<u64, String> {
    if !value.is_finite() || value < 0.0 {
        return Err("criterion sample duration must be finite and non-negative".to_string());
    }

    format!("{:.0}", value.ceil())
        .parse::<u64>()
        .map_err(|error| error.to_string())
}

fn percentile_us(sorted: &[u64], numerator: usize, denominator: usize) -> u64 {
    let index = sorted
        .len()
        .saturating_sub(1)
        .saturating_mul(numerator)
        .div_ceil(denominator);
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
