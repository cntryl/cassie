use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::{
    benchmark_for_scenario, benchmark_scenarios, deployment_profile_for_scenario,
    BenchmarkSampleSummary, BenchmarkTimingMode, PerformanceBenchmarkScenario, ResultCachePolicy,
    StressArtifactRowSummary, REQUIRED_WORKLOAD_FAMILIES,
};

const STRESS_SCHEMA_V1: &str = "cntryl-stress.v1";
const STRESS_SCHEMA_V2: &str = "cntryl-stress.v2";
const OPTIMIZATION_SIGNAL_ROLE: &str = "optimization";
const COMPLETE_SUITE_MAX_TIER_ONE_THROUGH_FOUR_NS: u64 = 900_000_000_000;
const COMPLETE_SUITE_MIN_TIER_SIX_SECONDS: f64 = 3_600.0;
const COMPLETE_SUITE_MIN_TIER_SIX_NS: u64 = 3_600_000_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkOwnerManifest {
    pub tier: u32,
    pub scenarios: BTreeSet<String>,
    pub result_cache_scenarios: BTreeSet<String>,
}

pub type BenchmarkSuiteManifest = BTreeMap<String, BenchmarkOwnerManifest>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct BenchmarkSuiteIdentity {
    run_id: String,
    git_commit: String,
    toolchain: String,
    profile: String,
}

pub fn artifact_output_dir(base: &Path, filtered_run: bool) -> PathBuf {
    if filtered_run {
        base.join("diagnostic")
    } else {
        base.to_path_buf()
    }
}

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

pub fn expected_complete_benchmark_manifest() -> Result<BenchmarkSuiteManifest, String> {
    let mut manifest = BenchmarkSuiteManifest::new();
    for scenario in benchmark_scenarios() {
        let owner = manifest
            .entry(scenario.benchmark.to_string())
            .or_insert_with(|| BenchmarkOwnerManifest {
                tier: scenario.declared_tier.number(),
                scenarios: BTreeSet::new(),
                result_cache_scenarios: BTreeSet::new(),
            });
        if owner.tier != scenario.declared_tier.number() {
            return Err(format!(
                "benchmark owner {} spans tiers {} and {}",
                scenario.benchmark,
                owner.tier,
                scenario.declared_tier.number()
            ));
        }
        if !owner.scenarios.insert(scenario.scenario_id.to_string()) {
            return Err(format!(
                "benchmark owner {} declares scenario {} more than once",
                scenario.benchmark, scenario.scenario_id
            ));
        }
        if scenario.result_cache_policy == ResultCachePolicy::Measured {
            owner
                .result_cache_scenarios
                .insert(scenario.scenario_id.to_string());
        }
    }
    if manifest.is_empty() {
        return Err("complete benchmark manifest has no expected owners".to_string());
    }
    let measured_result_cache_scenarios = manifest
        .values()
        .flat_map(|owner| {
            owner
                .result_cache_scenarios
                .iter()
                .map(move |scenario_id| (owner.tier, scenario_id))
        })
        .collect::<Vec<_>>();
    if measured_result_cache_scenarios.len() != 1 || measured_result_cache_scenarios[0].0 != 2 {
        return Err(
            "complete benchmark manifest must declare exactly one dedicated Tier 2 result-cache scenario"
                .to_string(),
        );
    }
    Ok(manifest)
}

pub fn validate_complete_benchmark_suite(stress_root: &Path) -> Result<(), String> {
    if stress_root.file_name().and_then(std::ffi::OsStr::to_str) == Some("diagnostic") {
        return Err("diagnostic benchmark paths cannot satisfy the complete suite".to_string());
    }

    let manifest = expected_complete_benchmark_manifest()?;
    let mut artifacts = BTreeMap::new();
    let entries = std::fs::read_dir(stress_root).map_err(|error| {
        format!(
            "cannot read canonical benchmark artifact root {}: {error}",
            stress_root.display()
        )
    })?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("cannot read benchmark artifact entry: {error}"))?;
        if !entry
            .file_type()
            .map_err(|error| format!("cannot inspect benchmark artifact entry: {error}"))?
            .is_dir()
        {
            continue;
        }
        let owner = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path().join("latest.json");
        if path.is_file() {
            let artifact = std::fs::read_to_string(&path).map_err(|error| {
                format!(
                    "cannot read canonical benchmark artifact {}: {error}",
                    path.display()
                )
            })?;
            artifacts.insert(owner, artifact);
        }
    }
    validate_complete_benchmark_artifacts(&manifest, &artifacts)
}

pub fn validate_complete_benchmark_artifacts(
    manifest: &BenchmarkSuiteManifest,
    artifacts: &BTreeMap<String, String>,
) -> Result<(), String> {
    let expected_owners = manifest.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let actual_owners = artifacts
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let missing_owners = expected_owners
        .difference(&actual_owners)
        .copied()
        .collect::<Vec<_>>();
    if !missing_owners.is_empty() {
        return Err(format!(
            "complete benchmark suite missing owners: {}",
            missing_owners.join(",")
        ));
    }
    let unexpected_owners = actual_owners
        .difference(&expected_owners)
        .copied()
        .collect::<Vec<_>>();
    if !unexpected_owners.is_empty() {
        return Err(format!(
            "complete benchmark suite has unexpected owners: {}",
            unexpected_owners.join(",")
        ));
    }

    let mut expected_identity = None;
    let mut tier_one_through_four_elapsed_ns = 0_u64;
    for (owner, owner_manifest) in manifest {
        let artifact_json = artifacts
            .get(owner)
            .ok_or_else(|| format!("complete benchmark suite missing owner {owner}"))?;
        let artifact: serde_json::Value = serde_json::from_str(artifact_json)
            .map_err(|error| format!("invalid benchmark artifact for {owner}: {error}"))?;
        let (identity, elapsed_ns) =
            validate_complete_owner_artifact(owner, owner_manifest, &artifact)?;
        if let Some(expected) = &expected_identity {
            validate_suite_identity(expected, &identity, owner)?;
        } else {
            expected_identity = Some(identity);
        }
        if (1..=4).contains(&owner_manifest.tier) {
            tier_one_through_four_elapsed_ns = tier_one_through_four_elapsed_ns
                .checked_add(elapsed_ns)
                .ok_or_else(|| "Tier 1-4 benchmark wall-time total overflowed".to_string())?;
        }
    }

    if tier_one_through_four_elapsed_ns > COMPLETE_SUITE_MAX_TIER_ONE_THROUGH_FOUR_NS {
        return Err(format!(
            "Tier 1-4 owner wall-time total {tier_one_through_four_elapsed_ns}ns exceeds 900 seconds"
        ));
    }
    Ok(())
}

fn validate_complete_owner_artifact(
    owner: &str,
    owner_manifest: &BenchmarkOwnerManifest,
    artifact: &serde_json::Value,
) -> Result<(BenchmarkSuiteIdentity, u64), String> {
    validate_schema_version(artifact)?;
    require_equal(artifact, "/suite", owner, "owner")?;
    require_metadata_flag(artifact, "filtered_run", false, owner)?;
    require_metadata_flag(artifact, "owner_suite_complete", true, owner)?;

    let identity = BenchmarkSuiteIdentity {
        run_id: required_nonempty_string(artifact, "/metadata/run_id", "CASSIE run ID")?,
        git_commit: required_nonempty_string(artifact, "/environment/git_commit", "git commit")?,
        toolchain: required_nonempty_string(artifact, "/environment/rustc_version", "toolchain")?,
        profile: required_nonempty_string(artifact, "/run_profile", "run profile")?,
    };
    if identity.profile == "smoke" {
        return Err("smoke profile artifacts cannot satisfy the complete suite".to_string());
    }

    let summaries = artifact["summaries"]
        .as_array()
        .ok_or_else(|| format!("benchmark owner {owner} summaries must be an array"))?;
    let mut actual_scenarios = BTreeSet::new();
    for summary in summaries {
        let metadata = &summary["metadata"];
        let scenario_id = required_metadata_string(metadata, "scenario_id")?;
        let summary_owner = required_metadata_string(metadata, "benchmark")?;
        if summary_owner != owner {
            return Err(format!(
                "benchmark owner {owner} contains summary owned by {summary_owner}"
            ));
        }
        let tier = summary["tier"].as_u64().ok_or_else(|| {
            format!("benchmark scenario {scenario_id} is missing its declared tier")
        })?;
        if tier != u64::from(owner_manifest.tier) {
            return Err(format!(
                "benchmark scenario {scenario_id} records Tier {tier}, expected Tier {}",
                owner_manifest.tier
            ));
        }
        actual_scenarios.insert(scenario_id);
    }

    let missing_scenarios = owner_manifest
        .scenarios
        .difference(&actual_scenarios)
        .map(String::as_str)
        .collect::<Vec<_>>();
    if !missing_scenarios.is_empty() {
        return Err(format!(
            "benchmark owner {owner} missing scenarios: {}",
            missing_scenarios.join(",")
        ));
    }
    let unexpected_scenarios = actual_scenarios
        .difference(&owner_manifest.scenarios)
        .map(String::as_str)
        .collect::<Vec<_>>();
    if !unexpected_scenarios.is_empty() {
        return Err(format!(
            "benchmark owner {owner} has unexpected scenarios: {}",
            unexpected_scenarios.join(",")
        ));
    }

    for summary in summaries {
        validate_registered_summary_contract(owner, owner_manifest, summary)?;
    }
    if owner_manifest.tier == 6 {
        validate_complete_tier_six_duration(owner, artifact, summaries)?;
    }

    let elapsed_ns = artifact["total_elapsed_ns"]
        .as_u64()
        .ok_or_else(|| format!("benchmark owner {owner} missing total_elapsed_ns"))?;
    Ok((identity, elapsed_ns))
}

fn validate_complete_tier_six_duration(
    owner: &str,
    artifact: &serde_json::Value,
    summaries: &[serde_json::Value],
) -> Result<(), String> {
    let metadata = &artifact["metadata"];
    let total_seconds = require_f64_metadata(metadata, "soak_total_duration_seconds")?;
    if total_seconds < COMPLETE_SUITE_MIN_TIER_SIX_SECONDS {
        return Err(format!(
            "Tier 6 owner {owner} records only {total_seconds} configured soak seconds; complete endurance evidence requires at least 3600"
        ));
    }
    let per_sample_seconds = require_f64_metadata(metadata, "soak_per_sample_duration_seconds")?;
    let measured_samples = require_numeric_metadata(metadata, "soak_measured_samples")?;
    if measured_samples == 0 || per_sample_seconds <= 0.0 {
        return Err(format!(
            "Tier 6 owner {owner} must record positive measured samples and per-sample duration"
        ));
    }
    let measured_samples = u32::try_from(measured_samples)
        .map_err(|_| format!("Tier 6 owner {owner} measured sample count exceeds u32"))?;
    let scheduled_seconds = per_sample_seconds * f64::from(measured_samples);
    if scheduled_seconds + 1.0e-6 < total_seconds {
        return Err(format!(
            "Tier 6 owner {owner} per-sample durations cover only {scheduled_seconds} of {total_seconds} configured seconds"
        ));
    }
    for summary in summaries {
        let scenario_id = required_metadata_string(&summary["metadata"], "scenario_id")?;
        let measured_wall_ns = summary["total_wall_clock_ns"].as_u64().ok_or_else(|| {
            format!("Tier 6 scenario {scenario_id} is missing total_wall_clock_ns")
        })?;
        if measured_wall_ns < COMPLETE_SUITE_MIN_TIER_SIX_NS {
            return Err(format!(
                "Tier 6 scenario {scenario_id} measured only {measured_wall_ns}ns; complete endurance evidence requires at least one hour"
            ));
        }
    }
    Ok(())
}

fn validate_registered_summary_contract(
    owner: &str,
    owner_manifest: &BenchmarkOwnerManifest,
    summary: &serde_json::Value,
) -> Result<(), String> {
    let metadata = &summary["metadata"];
    let scenario_id = required_metadata_string(metadata, "scenario_id")?;
    let scenario = benchmark_for_scenario(&scenario_id).ok_or_else(|| {
        format!("benchmark scenario {scenario_id} is not present in the registered catalog")
    })?;

    if scenario.benchmark != owner {
        return Err(format!(
            "benchmark scenario {scenario_id} registry owner mismatch: {} != {owner}",
            scenario.benchmark
        ));
    }
    if scenario.declared_tier.number() != owner_manifest.tier {
        return Err(format!(
            "benchmark scenario {scenario_id} registry tier mismatch: {} != {}",
            scenario.declared_tier.number(),
            owner_manifest.tier
        ));
    }

    require_scenario_metadata(metadata, &scenario_id, "benchmark", scenario.benchmark)?;
    require_scenario_metadata(metadata, &scenario_id, "workload", scenario.workload)?;
    require_scenario_metadata(
        metadata,
        &scenario_id,
        "fixture_scale",
        scenario.fixture_scale,
    )?;
    let fixture_rows = require_numeric_metadata(metadata, "fixture_rows")?;
    let expected_fixture_rows = u64::try_from(scenario.fixture_rows)
        .map_err(|_| format!("benchmark scenario {scenario_id} fixture_rows do not fit u64"))?;
    if fixture_rows != expected_fixture_rows {
        return Err(format!(
            "benchmark scenario {scenario_id} metadata.fixture_rows mismatch: {fixture_rows} != {expected_fixture_rows}"
        ));
    }
    let fixture_class = format!("{:?}", scenario.fixture_class).to_ascii_lowercase();
    require_scenario_metadata(metadata, &scenario_id, "fixture_class", &fixture_class)?;
    require_scenario_metadata(
        metadata,
        &scenario_id,
        "operation_unit",
        scenario.operation_unit,
    )?;
    require_scenario_metadata(
        metadata,
        &scenario_id,
        "signal_role",
        scenario.evidence_role.signal_role(),
    )?;
    let configured_worker_count = require_numeric_metadata(metadata, "configured_worker_count")?;
    let expected_worker_count = u64::from(scenario.worker_count.unwrap_or(0));
    if configured_worker_count != expected_worker_count {
        return Err(format!(
            "benchmark scenario {scenario_id} configured worker count mismatch: {configured_worker_count} != {expected_worker_count}"
        ));
    }

    let expected_intent = timing_intent(scenario.timing_mode);
    let actual_intent = summary["intent"].as_str().unwrap_or_default();
    if actual_intent != expected_intent {
        return Err(format!(
            "benchmark scenario {scenario_id} timing mode mismatch: artifact intent {actual_intent:?} != {expected_intent:?}"
        ));
    }

    let measured_result_cache = scenario.result_cache_policy == ResultCachePolicy::Measured;
    let manifest_measured_result_cache =
        owner_manifest.result_cache_scenarios.contains(&scenario_id);
    if measured_result_cache != manifest_measured_result_cache {
        return Err(format!(
            "benchmark scenario {scenario_id} result-cache policy disagrees with the registered catalog"
        ));
    }
    validate_query_evidence(summary, measured_result_cache, Some(scenario))?;
    if measured_result_cache
        && require_numeric_metadata(metadata, "execution_result_cache_hits")? == 0
    {
        return Err(format!(
            "benchmark scenario {scenario_id} measured result-cache policy observed zero hits"
        ));
    }
    Ok(())
}

fn require_scenario_metadata(
    metadata: &serde_json::Value,
    scenario_id: &str,
    key: &str,
    expected: &str,
) -> Result<(), String> {
    let actual = required_metadata_string(metadata, key)?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "benchmark scenario {scenario_id} metadata.{key} mismatch: {actual:?} != {expected:?}"
        ))
    }
}

const fn timing_intent(timing_mode: BenchmarkTimingMode) -> &'static str {
    match timing_mode {
        BenchmarkTimingMode::Micro | BenchmarkTimingMode::Measure => "general",
        BenchmarkTimingMode::Counted | BenchmarkTimingMode::External => "external",
        BenchmarkTimingMode::Batch => "batch",
    }
}

fn validate_suite_identity(
    expected: &BenchmarkSuiteIdentity,
    actual: &BenchmarkSuiteIdentity,
    owner: &str,
) -> Result<(), String> {
    for (label, expected_value, actual_value) in [
        ("run ID", expected.run_id.as_str(), actual.run_id.as_str()),
        (
            "git commit",
            expected.git_commit.as_str(),
            actual.git_commit.as_str(),
        ),
        (
            "toolchain",
            expected.toolchain.as_str(),
            actual.toolchain.as_str(),
        ),
        (
            "profile",
            expected.profile.as_str(),
            actual.profile.as_str(),
        ),
    ] {
        if expected_value != actual_value {
            return Err(format!(
                "benchmark owner {owner} has mixed {label}: {actual_value:?} != {expected_value:?}"
            ));
        }
    }
    Ok(())
}

fn required_nonempty_string(
    artifact: &serde_json::Value,
    pointer: &str,
    label: &str,
) -> Result<String, String> {
    artifact
        .pointer(pointer)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| format!("complete benchmark artifact missing nonempty {label}"))
}

fn require_metadata_flag(
    artifact: &serde_json::Value,
    key: &str,
    expected: bool,
    owner: &str,
) -> Result<(), String> {
    let actual = artifact["metadata"][key].as_bool().or_else(|| {
        artifact["metadata"][key]
            .as_str()
            .and_then(|value| value.parse().ok())
    });
    if actual == Some(expected) {
        Ok(())
    } else {
        Err(format!(
            "benchmark owner {owner} metadata.{key} must be {expected}"
        ))
    }
}

pub fn validate_complete_benchmark_contract(
    artifact_json: &str,
    expected_commit: &str,
    expected_toolchain: &str,
    expected_profile: &str,
    expected_scenarios: &[&str],
) -> Result<(), String> {
    let artifact: serde_json::Value =
        serde_json::from_str(artifact_json).map_err(|error| error.to_string())?;
    validate_schema_version(&artifact)?;
    require_equal(
        &artifact,
        "/environment/git_commit",
        expected_commit,
        "git commit",
    )?;
    require_equal(
        &artifact,
        "/environment/rustc_version",
        expected_toolchain,
        "toolchain",
    )?;
    require_equal(&artifact, "/run_profile", expected_profile, "run profile")?;
    if artifact
        .pointer("/metadata/filtered_run")
        .and_then(serde_json::Value::as_str)
        != Some("false")
    {
        return Err("filtered benchmark run cannot satisfy an owner contract".to_string());
    }

    let summaries = artifact["summaries"]
        .as_array()
        .ok_or_else(|| "stress artifact summaries must be an array".to_string())?;
    let actual = summaries
        .iter()
        .filter_map(|summary| summary["metadata"]["scenario_id"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let missing = expected_scenarios
        .iter()
        .copied()
        .filter(|scenario| !actual.contains(scenario))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "owner artifact missing scenarios: {}",
            missing.join(",")
        ));
    }

    for summary in summaries
        .iter()
        .filter(|summary| summary["metadata"]["signal_role"].as_str() != Some("informational"))
    {
        validate_query_evidence(summary, false, None)?;
    }
    Ok(())
}

fn require_equal(
    artifact: &serde_json::Value,
    pointer: &str,
    expected: &str,
    label: &str,
) -> Result<(), String> {
    let actual = artifact
        .pointer(pointer)
        .and_then(serde_json::Value::as_str);
    if actual == Some(expected) {
        Ok(())
    } else {
        Err(format!(
            "benchmark {label} mismatch: {actual:?} != {expected}"
        ))
    }
}

fn validate_query_evidence(
    summary: &serde_json::Value,
    allows_result_cache_hits: bool,
    scenario: Option<&PerformanceBenchmarkScenario>,
) -> Result<(), String> {
    let metadata = &summary["metadata"];
    for key in [
        "selected_access_path",
        "access_path_evidence_source",
        "fallback_reason",
        "fallback_evidence_source",
        "worker_leak_evidence_source",
    ] {
        if metadata[key].as_str().is_none_or(str::is_empty) {
            return Err(format!("benchmark summary missing metadata.{key}"));
        }
    }
    for key in [
        "result_cardinality",
        "storage_reads",
        "candidate_count",
        "peak_query_memory_bytes",
        "worker_count",
        "configured_worker_count",
        "leaked_active_operator_workers",
        "setup_time_ns",
        "measurement_time_ns",
    ] {
        require_numeric_metadata(metadata, key)?;
    }
    let result_cache_hits = require_numeric_metadata(metadata, "execution_result_cache_hits")?;
    if !allows_result_cache_hits && result_cache_hits != 0 {
        return Err("execution result cache warmed a timed benchmark query".to_string());
    }
    let failed_operations = require_numeric_metadata(metadata, "failed_operations")?;
    if failed_operations != 0 {
        return Err("benchmark summary recorded failed operations".to_string());
    }
    let leaked_workers = require_numeric_metadata(metadata, "leaked_active_operator_workers")?;
    if leaked_workers != 0 {
        return Err("benchmark summary recorded leaked active operator workers".to_string());
    }
    if let Some(scenario) = scenario.filter(|scenario| scenario.requires_observed_query_evidence())
    {
        let access_source = metadata["access_path_evidence_source"]
            .as_str()
            .unwrap_or_default();
        if !matches!(access_source, "preflight" | "operation") {
            return Err(format!(
                "benchmark scenario {} requires observed selected_access_path evidence",
                scenario.scenario_id
            ));
        }
        if let Some(expected) = scenario.expected_selected_access_path() {
            let observed = metadata["selected_access_path"]
                .as_str()
                .unwrap_or_default();
            if observed != expected {
                return Err(format!(
                    "benchmark scenario {} selected access path mismatch: observed '{observed}', expected '{expected}'",
                    scenario.scenario_id
                ));
            }
            if access_source != "preflight" {
                return Err(format!(
                    "benchmark scenario {} requires preflight selected access path evidence",
                    scenario.scenario_id
                ));
            }
        }
        let fallback_source = metadata["fallback_evidence_source"]
            .as_str()
            .unwrap_or_default();
        if !matches!(
            fallback_source,
            "preflight" | "runtime_metrics" | "operation"
        ) {
            return Err(format!(
                "benchmark scenario {} requires observed fallback_reason evidence",
                scenario.scenario_id
            ));
        }
        if metadata["worker_leak_evidence_source"].as_str() != Some("runtime_metrics") {
            return Err(format!(
                "benchmark scenario {} requires observed worker-leak evidence",
                scenario.scenario_id
            ));
        }
    }
    Ok(())
}

fn require_numeric_metadata(metadata: &serde_json::Value, key: &str) -> Result<u64, String> {
    metadata[key]
        .as_u64()
        .or_else(|| metadata[key].as_str().and_then(|value| value.parse().ok()))
        .ok_or_else(|| format!("benchmark summary missing numeric metadata.{key}"))
}

fn require_f64_metadata(metadata: &serde_json::Value, key: &str) -> Result<f64, String> {
    metadata[key]
        .as_f64()
        .or_else(|| metadata[key].as_str().and_then(|value| value.parse().ok()))
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("benchmark summary missing finite numeric metadata.{key}"))
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
