#[path = "performance_benchmark_profiles.rs"]
mod performance_benchmark_profiles;
#[path = "performance_benchmark_samples.rs"]
mod performance_benchmark_samples;
#[path = "performance_benchmark_scenarios.rs"]
mod performance_benchmark_scenarios;
#[path = "performance_benchmark_types.rs"]
mod performance_benchmark_types;

pub type BenchmarkSampleSummary = performance_benchmark_types::BenchmarkSampleSummary;
pub type BenchmarkOwnerManifest = performance_benchmark_samples::BenchmarkOwnerManifest;
pub type BenchmarkSuiteManifest = performance_benchmark_samples::BenchmarkSuiteManifest;
pub type BenchmarkTier = performance_benchmark_types::BenchmarkTier;
pub type BenchmarkTimingMode = performance_benchmark_types::BenchmarkTimingMode;
pub type DeploymentProfile = performance_benchmark_types::DeploymentProfile;
pub type EvidenceRole = performance_benchmark_types::EvidenceRole;
pub type FixtureClass = performance_benchmark_types::FixtureClass;
pub type PerformanceBenchmarkScenario = performance_benchmark_types::PerformanceBenchmarkScenario;
pub type ResultCachePolicy = performance_benchmark_types::ResultCachePolicy;
pub type StressArtifactRowSummary = performance_benchmark_types::StressArtifactRowSummary;

pub const DEPLOYMENT_PROFILES: &[DeploymentProfile] =
    performance_benchmark_profiles::DEPLOYMENT_PROFILES;
pub const REQUIRED_WORKLOAD_FAMILIES: &[&str] =
    performance_benchmark_scenarios::REQUIRED_WORKLOAD_FAMILIES;
pub const SUPPORTED_SCALES: &[&str] = performance_benchmark_scenarios::SUPPORTED_SCALES;

pub fn benchmark_scenarios() -> impl Iterator<Item = &'static PerformanceBenchmarkScenario> {
    performance_benchmark_scenarios::benchmark_scenarios()
}

/// # Errors
///
/// Returns an error when a registered scenario violates its tier ownership,
/// timing, or fixture policy.
pub fn validate_scenario_contract(scenario: &PerformanceBenchmarkScenario) -> Result<(), String> {
    if !scenario
        .benchmark
        .starts_with(scenario.declared_tier.owner_prefix())
    {
        return Err(format!(
            "owner {} does not match Tier {}",
            scenario.benchmark,
            scenario.declared_tier.number()
        ));
    }

    let timing_valid = match scenario.declared_tier {
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
    };
    if !timing_valid {
        return Err(format!(
            "scenario {} has invalid {:?} timing for Tier {}",
            scenario.scenario_id,
            scenario.timing_mode,
            scenario.declared_tier.number()
        ));
    }

    let fixture_valid = match scenario.declared_tier {
        BenchmarkTier::Tier1 => {
            scenario.fixture_class == FixtureClass::Kernel && scenario.fixture_rows == 0
        }
        BenchmarkTier::Tier2 => {
            scenario.fixture_class == FixtureClass::Subsystem && scenario.fixture_rows <= 2_048
        }
        BenchmarkTier::Tier3 => {
            scenario.fixture_class == FixtureClass::Representative
                && scenario.fixture_rows == 100_000
        }
        BenchmarkTier::Tier4 => {
            scenario.fixture_class == FixtureClass::Integration && scenario.fixture_rows == 10_000
        }
        BenchmarkTier::Tier5 => {
            scenario.fixture_class == FixtureClass::Scaling
                && matches!(scenario.fixture_rows, 10_000 | 100_000 | 250_000)
        }
        BenchmarkTier::Tier6 => {
            scenario.fixture_class == FixtureClass::Soak
                && matches!(scenario.fixture_rows, 10_000 | 100_000)
        }
    };
    if !fixture_valid {
        return Err(format!(
            "scenario {} has invalid {:?}/{} fixture for Tier {}",
            scenario.scenario_id,
            scenario.fixture_class,
            scenario.fixture_rows,
            scenario.declared_tier.number()
        ));
    }
    Ok(())
}

#[must_use]
pub fn benchmark_for_scenario(scenario_id: &str) -> Option<&'static PerformanceBenchmarkScenario> {
    benchmark_scenarios().find(|benchmark| benchmark.scenario_id == scenario_id)
}

#[must_use]
pub fn benchmark_for_benchmark(
    benchmark_name: &str,
    workload: &str,
    fixture_scale: &str,
) -> Option<&'static PerformanceBenchmarkScenario> {
    benchmark_scenarios().find(|scenario| {
        scenario.benchmark == benchmark_name
            && scenario.workload == workload
            && scenario.fixture_scale == fixture_scale
    })
}

/// # Panics
///
/// Panics when no registered benchmark scenario matches the benchmark, workload,
/// and fixture scale.
#[must_use]
pub fn expect_benchmark(
    benchmark: &str,
    workload: &str,
    fixture_scale: &str,
) -> &'static PerformanceBenchmarkScenario {
    benchmark_for_benchmark(benchmark, workload, fixture_scale).unwrap_or_else(|| {
        panic!("missing performance benchmark for {benchmark}/{workload}/{fixture_scale}")
    })
}

#[must_use]
pub fn deployment_profile_for_id(profile_id: &str) -> Option<&'static DeploymentProfile> {
    performance_benchmark_profiles::deployment_profile_for_id(profile_id)
}

#[must_use]
pub fn deployment_profile_for_scenario(
    benchmark: &PerformanceBenchmarkScenario,
) -> Option<&'static DeploymentProfile> {
    performance_benchmark_profiles::deployment_profile_for_scenario(benchmark)
}

#[must_use]
pub fn expected_stress_artifact_path(
    stress_root: &std::path::Path,
    benchmark: &PerformanceBenchmarkScenario,
) -> std::path::PathBuf {
    performance_benchmark_samples::expected_stress_artifact_path(stress_root, benchmark)
}

#[must_use]
pub fn artifact_output_dir(base: &std::path::Path, filtered_run: bool) -> std::path::PathBuf {
    performance_benchmark_samples::artifact_output_dir(base, filtered_run)
}

/// # Errors
///
/// Returns an error when the artifact is not valid stress JSON, uses an
/// unsupported schema version, or does not contain the expected benchmark row.
pub fn summarize_stress_artifact(
    benchmark: &PerformanceBenchmarkScenario,
    artifact_json: &str,
) -> Result<BenchmarkSampleSummary, String> {
    performance_benchmark_samples::summarize_stress_artifact(benchmark, artifact_json)
}

/// # Errors
///
/// Returns an error when the artifact is not valid stress JSON, uses an
/// unsupported schema version, or contains rows that cannot be summarized.
pub fn summarize_stress_artifact_rows(
    artifact_json: &str,
) -> Result<Vec<StressArtifactRowSummary>, String> {
    performance_benchmark_samples::summarize_stress_artifact_rows(artifact_json)
}

/// # Errors
///
/// Returns an error when optimization rows are missing required ownership
/// metadata or when artifact rows cannot be parsed.
pub fn validate_stress_artifact_signal_metadata(artifact_json: &str) -> Result<(), String> {
    performance_benchmark_samples::validate_stress_artifact_signal_metadata(artifact_json)
}

/// # Errors
///
/// Returns an error when an artifact cannot serve as complete benchmark-owner evidence.
pub fn validate_complete_benchmark_contract(
    artifact_json: &str,
    expected_commit: &str,
    expected_toolchain: &str,
    expected_profile: &str,
    expected_scenarios: &[&str],
) -> Result<(), String> {
    performance_benchmark_samples::validate_complete_benchmark_contract(
        artifact_json,
        expected_commit,
        expected_toolchain,
        expected_profile,
        expected_scenarios,
    )
}

/// # Errors
///
/// Returns an error when the scenario registry cannot form one exact owner manifest.
pub fn expected_complete_benchmark_manifest() -> Result<BenchmarkSuiteManifest, String> {
    performance_benchmark_samples::expected_complete_benchmark_manifest()
}

/// # Errors
///
/// Returns an error when owner artifacts do not form one complete canonical benchmark suite.
pub fn validate_complete_benchmark_artifacts(
    manifest: &BenchmarkSuiteManifest,
    artifacts: &std::collections::BTreeMap<String, String>,
) -> Result<(), String> {
    performance_benchmark_samples::validate_complete_benchmark_artifacts(manifest, artifacts)
}

/// # Errors
///
/// Returns an error when canonical owner artifacts are missing or violate the suite manifest.
pub fn validate_complete_benchmark_suite(stress_root: &std::path::Path) -> Result<(), String> {
    performance_benchmark_samples::validate_complete_benchmark_suite(stress_root)
}
