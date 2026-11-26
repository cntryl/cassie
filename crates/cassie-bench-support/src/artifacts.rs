use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::performance_benchmarks::{benchmark_scenarios, summarize_stress_artifact_rows};
use crate::registry::{
    registered_targets, select_targets, validate_scenario_registry, BenchmarkTier, RunCategory,
    TargetSelection,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunManifest {
    pub schema_version: u32,
    pub suite_id: String,
    pub run_id: String,
    pub scope: RunScope,
    pub provenance: RunProvenance,
    pub artifacts: Vec<ArtifactReference>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunScope {
    pub tiers: Vec<BenchmarkTier>,
    pub categories: Vec<RunCategory>,
    #[serde(default)]
    pub targets: Vec<String>,
    pub profile: String,
    #[serde(default = "default_run_profile")]
    pub run_profile: String,
    #[serde(default)]
    pub require_full_soaks: bool,
}

fn default_run_profile() -> String {
    "full".to_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunProvenance {
    pub commit: String,
    pub toolchain: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostIdentity {
    pub hardware_class: String,
    pub os: String,
    pub architecture: String,
    pub job_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactReference {
    pub package: String,
    pub target: String,
    pub tier: BenchmarkTier,
    pub category: RunCategory,
    pub profile: String,
    pub commit: String,
    pub toolchain: String,
    pub host: HostIdentity,
    pub elapsed_seconds: f64,
    pub raw_path: String,
}

#[derive(Debug, Serialize)]
pub struct ValidationSummary {
    pub suite_id: String,
    pub run_id: String,
    pub scope: RunScope,
    pub expected_cases: usize,
    pub observed_cases: usize,
    pub artifacts: usize,
    pub timed_tier_1_to_4_seconds: f64,
    pub suite_complete: bool,
    pub measurement_valid: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedScenarioCase {
    pub target: String,
    pub tier: BenchmarkTier,
    pub fixture_scale: String,
}

/// Derives expected case IDs from the compiled catalog and selected run scope.
///
/// # Panics
///
/// Panics when a catalog scenario contains a tier outside the supported range.
#[must_use]
pub fn expected_scenario_cases(
    scope: &RunScope,
    selected_targets: &BTreeSet<String>,
) -> BTreeMap<String, ExpectedScenarioCase> {
    let mut expected = BTreeMap::new();
    for scenario in benchmark_scenarios() {
        if selected_targets.contains(scenario.benchmark) {
            expected.insert(
                scenario.scenario_id.to_owned(),
                ExpectedScenarioCase {
                    target: scenario.benchmark.to_owned(),
                    tier: BenchmarkTier::from_number(scenario.declared_tier.number())
                        .expect("scenario tier must be in range 1 through 6"),
                    fixture_scale: scenario.fixture_scale.to_owned(),
                },
            );
        }
    }
    if selected_targets.contains("tier5_time_series_mixed") {
        for workload in ["ingest_only", "reader_only", "mixed"] {
            for axis in ["16", "1024"] {
                if scope.run_profile == "smoke" {
                    let id =
                        format!("perf.time_series.mixed_workload.{workload}.smoke.{axis}devices");
                    expected.insert(
                        id,
                        ExpectedScenarioCase {
                            target: "tier5_time_series_mixed".to_owned(),
                            tier: BenchmarkTier::Tier5,
                            fixture_scale: "smoke".to_owned(),
                        },
                    );
                } else {
                    for scale in ["100k", "250k"] {
                        let id = format!(
                            "perf.time_series.mixed_workload.{workload}.{scale}.{axis}devices"
                        );
                        expected.insert(
                            id,
                            ExpectedScenarioCase {
                                target: "tier5_time_series_mixed".to_owned(),
                                tier: BenchmarkTier::Tier5,
                                fixture_scale: scale.to_owned(),
                            },
                        );
                    }
                }
            }
        }
    }
    expected
}

/// Validates a suite run against the compiled target and scenario registries.
///
/// `raw_artifacts` maps the manifest's raw paths to their contents, which keeps
/// validation independent of the caller's filesystem layout.
///
/// # Errors
///
/// Returns an error for incomplete or mixed target, case, timing, profile, host, or provenance evidence.
pub fn validate_run_manifest(
    manifest: &RunManifest,
    targets: &[crate::registry::Target],
    raw_artifacts: &BTreeMap<String, String>,
) -> Result<ValidationSummary, String> {
    let scenarios = benchmark_scenarios().copied().collect::<Vec<_>>();
    validate_scenario_registry(targets, &scenarios)?;
    if manifest.schema_version != 1 {
        return Err(format!(
            "unsupported run manifest schema_version {}",
            manifest.schema_version
        ));
    }
    if manifest.scope.run_profile != "smoke" && manifest.scope.run_profile != "full" {
        return Err(format!(
            "unsupported run_profile {}",
            manifest.scope.run_profile
        ));
    }
    if manifest.scope.profile != "native-linux-arm64-disk" {
        return Err(format!(
            "new benchmark qualification requires native-linux-arm64-disk, got {}",
            manifest.scope.profile
        ));
    }
    if manifest.suite_id.is_empty()
        || manifest.run_id.is_empty()
        || manifest.scope.profile.is_empty()
    {
        return Err("suite_id, run_id, and scope.profile must be non-empty".to_owned());
    }
    let selection = TargetSelection {
        tiers: manifest.scope.tiers.clone(),
        categories: manifest.scope.categories.clone(),
        targets: manifest.scope.targets.clone(),
    };
    let selected = select_targets(targets, &selection);
    if selected.is_empty() {
        return Err("run scope selects no registered targets".to_owned());
    }
    if selected
        .iter()
        .any(|target| target.category != RunCategory::Timed)
    {
        return Err(
            "timed suite validator cannot include product or diagnostic targets".to_owned(),
        );
    }
    let expected_targets = selected
        .iter()
        .map(|target| target.target.as_str())
        .collect::<BTreeSet<_>>();
    let mut actual_targets = BTreeSet::new();
    let mut all_ids = BTreeSet::new();
    let expected_cases = expected_scenario_cases(
        &manifest.scope,
        &expected_targets
            .iter()
            .map(|target| (*target).to_owned())
            .collect(),
    );
    let expected_ids = expected_cases.keys().cloned().collect::<BTreeSet<_>>();
    if expected_ids.is_empty() {
        return Err("run scope has no registered scenario axes".to_owned());
    }

    let mut tier_1_to_4_seconds = 0.0_f64;
    let mut tier6_seconds = BTreeMap::<String, f64>::new();
    let mut seen_jobs = BTreeSet::new();
    let mut architecture = None::<String>;
    let mut hardware_class = None::<String>;
    for artifact in &manifest.artifacts {
        if artifact.category != RunCategory::Timed {
            return Err(format!("artifact {} has non-timed category {:?}; product and diagnostic evidence cannot satisfy timed scopes", artifact.target, artifact.category));
        }
        let target = selected
            .iter()
            .find(|target| target.target == artifact.target)
            .ok_or_else(|| {
                format!(
                    "artifact target {} is outside the selected registry scope",
                    artifact.target
                )
            })?;
        if target.package != artifact.package
            || target.tier != artifact.tier
            || target.category != artifact.category
        {
            return Err(format!(
                "artifact registration mismatch for {}/{}",
                artifact.package, artifact.target
            ));
        }
        if artifact.profile != manifest.scope.profile
            || artifact.commit != manifest.provenance.commit
            || artifact.toolchain != manifest.provenance.toolchain
        {
            return Err(format!(
                "mixed profile or provenance in artifact {}",
                artifact.target
            ));
        }
        if artifact.host.hardware_class.is_empty()
            || artifact.host.os.is_empty()
            || artifact.host.architecture.is_empty()
            || artifact.host.job_id.is_empty()
        {
            return Err(format!(
                "artifact {} has incomplete host/job identity",
                artifact.target
            ));
        }
        if artifact.host.os != "linux" || artifact.host.architecture != "aarch64" {
            return Err(format!(
                "artifact {} is not from the required native Linux aarch64 host",
                artifact.target
            ));
        }
        if architecture.get_or_insert_with(|| artifact.host.architecture.clone())
            != &artifact.host.architecture
        {
            return Err("run contains mixed host architectures".to_owned());
        }
        if hardware_class.get_or_insert_with(|| artifact.host.hardware_class.clone())
            != &artifact.host.hardware_class
        {
            return Err("run contains mixed host hardware classes".to_owned());
        }
        if !artifact.elapsed_seconds.is_finite() || artifact.elapsed_seconds < 0.0 {
            return Err(format!(
                "artifact {} has invalid elapsed_seconds",
                artifact.target
            ));
        }
        if !actual_targets.insert(artifact.target.as_str()) {
            return Err(format!("duplicate artifact target {}", artifact.target));
        }
        if !seen_jobs.insert((artifact.target.as_str(), artifact.host.job_id.as_str())) {
            return Err(format!(
                "duplicate artifact job {} for target {}",
                artifact.host.job_id, artifact.target
            ));
        }
        let raw = raw_artifacts
            .get(&artifact.raw_path)
            .ok_or_else(|| format!("missing raw artifact {}", artifact.raw_path))?;
        let parsed: serde_json::Value = serde_json::from_str(raw)
            .map_err(|error| format!("invalid raw artifact {}: {error}", artifact.raw_path))?;
        let metadata = &parsed["metadata"];
        for (key, expected) in [
            ("source_commit", artifact.commit.as_str()),
            ("toolchain", artifact.toolchain.as_str()),
            ("host_hardware_class", artifact.host.hardware_class.as_str()),
            ("host_os", artifact.host.os.as_str()),
            ("host_architecture", artifact.host.architecture.as_str()),
            ("job_id", artifact.host.job_id.as_str()),
        ] {
            if metadata[key].as_str() != Some(expected) {
                return Err(format!(
                    "raw artifact {}/{} is missing matching metadata.{key}",
                    artifact.target, artifact.raw_path
                ));
            }
        }
        let filtered_run = metadata["filtered_run"].as_bool();
        if (manifest.scope.run_profile == "smoke" && filtered_run != Some(true))
            || (manifest.scope.run_profile != "smoke" && filtered_run != Some(false))
        {
            return Err(format!(
                "artifact {} filtered_run marker does not match run_profile",
                artifact.target
            ));
        }
        if metadata["deployment_profile_id"].as_str() != Some(manifest.scope.profile.as_str()) {
            return Err(format!(
                "raw artifact profile mismatch for {}",
                artifact.target
            ));
        }
        if metadata["run_id"].as_str() != Some(manifest.run_id.as_str()) {
            return Err(format!(
                "raw artifact run_id mismatch for {}",
                artifact.target
            ));
        }
        if manifest.scope.run_profile != "smoke"
            && metadata["owner_suite_complete"].as_bool() != Some(true)
        {
            return Err(format!(
                "artifact {} is marked as an incomplete owner suite",
                artifact.target
            ));
        }
        let rows = summarize_stress_artifact_rows(raw)?;
        if rows.is_empty() {
            return Err(format!(
                "raw artifact {} contains no cases",
                artifact.target
            ));
        }
        for row in rows {
            if row.tier != u64::from(artifact.tier.number()) {
                return Err(format!(
                    "case {} has wrong tier in {}",
                    row.workload, artifact.target
                ));
            }
            let id = row.scenario_id.ok_or_else(|| {
                format!(
                    "case {}/{} is missing scenario_id",
                    artifact.target, row.workload
                )
            })?;
            let expected = expected_cases.get(&id);
            if expected.is_none_or(|case| {
                case.target != artifact.target
                    || case.fixture_scale != row.fixture_scale
                    || case.tier != artifact.tier
            }) {
                return Err(format!(
                    "unexpected case {id} in artifact {}",
                    artifact.target
                ));
            }
            if !all_ids.insert(id.clone()) {
                return Err(format!("duplicate case {id}"));
            }
        }
        if (1..=4).contains(&artifact.tier.number()) {
            tier_1_to_4_seconds += artifact.elapsed_seconds;
        }
        if artifact.tier == BenchmarkTier::Tier6 {
            if manifest.scope.run_profile != "smoke" {
                let measured_seconds = metadata["soak_total_duration_seconds"]
                    .as_f64()
                    .ok_or_else(|| {
                        format!(
                            "Tier 6 artifact {} is missing measured soak duration",
                            artifact.target
                        )
                    })?;
                if measured_seconds < 3600.0 {
                    return Err(format!(
                        "Tier 6 artifact {} is shorter than 3600 measured seconds",
                        artifact.target
                    ));
                }
            }
            tier6_seconds.insert(artifact.target.clone(), artifact.elapsed_seconds);
        }
    }
    if actual_targets != expected_targets {
        let missing = expected_targets
            .difference(&actual_targets)
            .copied()
            .collect::<Vec<_>>();
        let unexpected = actual_targets
            .difference(&expected_targets)
            .copied()
            .collect::<Vec<_>>();
        return Err(format!(
            "target coverage mismatch; missing [{}], unexpected [{}]",
            missing.join(","),
            unexpected.join(",")
        ));
    }
    if all_ids != expected_ids {
        let missing = expected_ids
            .difference(&all_ids)
            .cloned()
            .collect::<Vec<_>>();
        return Err(format!("missing expected scenarios: {}", missing.join(",")));
    }
    if tier_1_to_4_seconds > 900.0 {
        return Err(format!(
            "Tier 1–4 aggregate duration exceeds 900 seconds: {tier_1_to_4_seconds:.3}"
        ));
    }
    if manifest.scope.require_full_soaks || manifest.scope.run_profile != "smoke" {
        for target in selected
            .iter()
            .filter(|target| target.tier == BenchmarkTier::Tier6)
        {
            if tier6_seconds
                .get(&target.target)
                .copied()
                .unwrap_or_default()
                < 3600.0
            {
                return Err(format!(
                    "Tier 6 target {} is shorter than 3600 measured seconds",
                    target.target
                ));
            }
        }
    }
    Ok(ValidationSummary {
        suite_id: manifest.suite_id.clone(),
        run_id: manifest.run_id.clone(),
        scope: manifest.scope.clone(),
        expected_cases: expected_ids.len(),
        observed_cases: all_ids.len(),
        artifacts: manifest.artifacts.len(),
        timed_tier_1_to_4_seconds: tier_1_to_4_seconds,
        suite_complete: true,
        measurement_valid: manifest.scope.run_profile != "smoke",
    })
}

/// Loads a manifest and its raw artifacts, then validates expected cases from the compiled registry.
///
/// # Errors
///
/// Returns an error when the manifest or any referenced artifact cannot be read or validated.
pub fn validate_run_manifest_file(path: &Path) -> Result<ValidationSummary, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let manifest: RunManifest = serde_json::from_str(&source).map_err(|error| error.to_string())?;
    let root = path.parent().unwrap_or_else(|| Path::new("."));
    let mut raw = BTreeMap::new();
    for artifact in &manifest.artifacts {
        let artifact_path = root.join(&artifact.raw_path);
        let content = fs::read_to_string(&artifact_path).map_err(|error| {
            format!(
                "cannot read raw artifact {}: {error}",
                artifact_path.display()
            )
        })?;
        raw.insert(artifact.raw_path.clone(), content);
    }
    validate_run_manifest(&manifest, &registered_targets(), &raw)
}
