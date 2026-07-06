use std::path::{Path, PathBuf};

use super::{
    deployment_profile_for_scenario, BenchmarkSampleSummary, PerformanceBenchmarkScenario,
    StressArtifactRowSummary, REQUIRED_WORKLOAD_FAMILIES,
};

const STRESS_SCHEMA_V1: &str = "cntryl-stress.v1";
const STRESS_SCHEMA_V2: &str = "cntryl-stress.v2";
const INFORMATIONAL_SIGNAL_ROLE: &str = "informational";
const OPTIMIZATION_SIGNAL_ROLE: &str = "optimization";

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
    validate_schema_version(&artifact)?;

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

pub fn summarize_stress_artifact_rows(
    artifact_json: &str,
) -> Result<Vec<StressArtifactRowSummary>, String> {
    let artifact: serde_json::Value =
        serde_json::from_str(artifact_json).map_err(|error| error.to_string())?;
    validate_schema_version(&artifact)?;
    let summaries = artifact["summaries"]
        .as_array()
        .ok_or_else(|| "stress artifact summaries must be an array".to_string())?;
    summaries.iter().map(row_summary).collect()
}

pub fn validate_stress_artifact_signal_metadata(artifact_json: &str) -> Result<(), String> {
    let rows = summarize_stress_artifact_rows(artifact_json)?;
    let families = REQUIRED_WORKLOAD_FAMILIES
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let failures = rows
        .iter()
        .filter(|row| (2..=4).contains(&row.tier) && row.is_optimization_signal())
        .filter_map(|row| required_metadata_failure(row, &families))
        .collect::<Vec<_>>();

    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

fn validate_schema_version(artifact: &serde_json::Value) -> Result<(), String> {
    let schema_version = artifact["schema_version"].as_str().unwrap_or_default();
    if matches!(schema_version, STRESS_SCHEMA_V1 | STRESS_SCHEMA_V2) {
        Ok(())
    } else {
        Err(format!("unsupported schema_version '{schema_version}'"))
    }
}

fn ceil_microseconds(value: f64) -> Result<u64, String> {
    if !value.is_finite() || value < 0.0 {
        return Err("stress sample duration must be finite and non-negative".to_string());
    }

    format!("{:.0}", value.ceil())
        .parse::<u64>()
        .map_err(|error| error.to_string())
}

fn row_summary(summary: &serde_json::Value) -> Result<StressArtifactRowSummary, String> {
    let metadata = &summary["metadata"];
    let benchmark = required_metadata_string(metadata, "benchmark")?;
    let workload = required_metadata_string(metadata, "workload")?;
    let fixture_scale = required_metadata_string(metadata, "fixture_scale")?;
    let tier = summary["tier"].as_u64().ok_or_else(|| {
        format!("stress summary {benchmark}/{workload}/{fixture_scale} missing tier")
    })?;
    let signal_role = metadata["signal_role"]
        .as_str()
        .unwrap_or(OPTIMIZATION_SIGNAL_ROLE)
        .to_string();

    Ok(StressArtifactRowSummary {
        benchmark,
        workload,
        fixture_scale,
        tier,
        scenario_id: optional_metadata_string(metadata, "scenario_id"),
        family: optional_metadata_string(metadata, "family"),
        signal_role,
        operation_unit: optional_metadata_string(metadata, "operation_unit"),
        logical_operations_per_iteration: logical_operations_per_iteration(summary),
        logical_operations_source: optional_metadata_string(metadata, "logical_operations_source"),
        diagnostic_codes: diagnostic_codes(summary)?,
    })
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

fn required_metadata_string(metadata: &serde_json::Value, key: &str) -> Result<String, String> {
    metadata[key]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| format!("stress summary missing metadata.{key}"))
}

fn optional_metadata_string(metadata: &serde_json::Value, key: &str) -> Option<String> {
    metadata[key]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn logical_operations_per_iteration(summary: &serde_json::Value) -> Option<u64> {
    parse_u64_string(&summary["metadata"]["logical_operations_per_iteration"])
        .or_else(|| parse_u64_string(&summary["parameters"]["logical_operations_per_iteration"]))
}

fn parse_u64_string(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<u64>().ok()))
}

fn diagnostic_codes(summary: &serde_json::Value) -> Result<Vec<String>, String> {
    match summary.get("diagnostics") {
        None | Some(serde_json::Value::Null) => Ok(Vec::new()),
        Some(serde_json::Value::Array(diagnostics)) => diagnostics
            .iter()
            .map(|diagnostic| {
                diagnostic["code"]
                    .as_str()
                    .filter(|code| !code.is_empty())
                    .map(ToString::to_string)
                    .ok_or_else(|| "stress diagnostic missing code".to_string())
            })
            .collect(),
        Some(_) => Err("stress diagnostics must be an array or null".to_string()),
    }
}

fn required_metadata_failure(
    row: &StressArtifactRowSummary,
    families: &std::collections::BTreeSet<&str>,
) -> Option<String> {
    let mut missing = Vec::new();
    if row.scenario_id.is_none() {
        missing.push("scenario_id");
    }
    if row.family.is_none() {
        missing.push("family");
    }
    if row.operation_unit.is_none() {
        missing.push("operation_unit");
    }
    if row.logical_operations_per_iteration.is_none() && row.logical_operations_source.is_none() {
        missing.push("logical_operations_per_iteration");
    }

    if let Some(family) = row.family.as_deref() {
        if !families.contains(family) {
            missing.push("valid_family");
        }
    }

    if missing.is_empty() {
        None
    } else {
        Some(format!(
            "{}/{}/{} missing {}",
            row.benchmark,
            row.workload,
            row.fixture_scale,
            missing.join(",")
        ))
    }
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
