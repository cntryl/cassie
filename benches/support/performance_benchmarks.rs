#[path = "performance_benchmark_profiles.rs"]
mod performance_benchmark_profiles;
#[path = "performance_benchmark_samples.rs"]
mod performance_benchmark_samples;
#[path = "performance_benchmark_scenarios.rs"]
mod performance_benchmark_scenarios;
#[path = "performance_benchmark_types.rs"]
mod performance_benchmark_types;

pub type BenchmarkSampleSummary = performance_benchmark_types::BenchmarkSampleSummary;
pub type DeploymentProfile = performance_benchmark_types::DeploymentProfile;
pub type PerformanceBenchmarkScenario = performance_benchmark_types::PerformanceBenchmarkScenario;
pub type StressArtifactRowSummary = performance_benchmark_types::StressArtifactRowSummary;

pub const DEPLOYMENT_PROFILES: &[DeploymentProfile] =
    performance_benchmark_profiles::DEPLOYMENT_PROFILES;
pub const REQUIRED_WORKLOAD_FAMILIES: &[&str] =
    performance_benchmark_scenarios::REQUIRED_WORKLOAD_FAMILIES;
pub const SUPPORTED_SCALES: &[&str] = performance_benchmark_scenarios::SUPPORTED_SCALES;

pub fn benchmark_scenarios() -> impl Iterator<Item = &'static PerformanceBenchmarkScenario> {
    performance_benchmark_scenarios::benchmark_scenarios()
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
