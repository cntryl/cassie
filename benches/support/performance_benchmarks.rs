#![allow(dead_code)]

#[path = "performance_benchmark_profiles.rs"]
mod performance_benchmark_profiles;
#[path = "performance_benchmark_samples.rs"]
mod performance_benchmark_samples;
#[path = "performance_benchmark_scenarios.rs"]
mod performance_benchmark_scenarios;
#[path = "performance_benchmark_types.rs"]
mod performance_benchmark_types;
#[path = "performance_benchmark_vector_scenarios.rs"]
mod performance_benchmark_vector_scenarios;

#[path = "performance_benchmark_placeholders.rs"]
mod performance_benchmark_placeholders;

pub type BenchmarkSampleSummary = performance_benchmark_types::BenchmarkSampleSummary;
pub type DeploymentProfile = performance_benchmark_types::DeploymentProfile;
pub type PerformanceBenchmarkScenario = performance_benchmark_types::PerformanceBenchmarkScenario;
pub type StressArtifactRowSummary = performance_benchmark_types::StressArtifactRowSummary;

pub const BENCHMARK_SCENARIOS: &[PerformanceBenchmarkScenario] =
    performance_benchmark_scenarios::BENCHMARK_SCENARIOS;
pub const BENCHMARK_SCENARIO_PLACEHOLDERS: &[PerformanceBenchmarkScenario] =
    performance_benchmark_placeholders::BENCHMARK_SCENARIO_PLACEHOLDERS;
pub const DEPLOYMENT_PROFILES: &[DeploymentProfile] =
    performance_benchmark_profiles::DEPLOYMENT_PROFILES;
pub const REQUIRED_WORKLOAD_FAMILIES: &[&str] =
    performance_benchmark_scenarios::REQUIRED_WORKLOAD_FAMILIES;
pub const SUPPORTED_SCALES: &[&str] = performance_benchmark_scenarios::SUPPORTED_SCALES;

pub fn benchmark_scenarios() -> impl Iterator<Item = &'static PerformanceBenchmarkScenario> {
    performance_benchmark_scenarios::BENCHMARK_SCENARIOS
        .iter()
        .chain(performance_benchmark_vector_scenarios::VECTOR_PATH_SCENARIOS)
}

pub fn benchmark_for_scenario(scenario_id: &str) -> Option<&'static PerformanceBenchmarkScenario> {
    benchmark_scenarios().find(|benchmark| benchmark.scenario_id == scenario_id)
}

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
    performance_benchmark_profiles::deployment_profile_for_id(profile_id)
}

pub fn deployment_profile_for_scenario(
    benchmark: &PerformanceBenchmarkScenario,
) -> Option<&'static DeploymentProfile> {
    performance_benchmark_profiles::deployment_profile_for_scenario(benchmark)
}

pub fn expected_stress_artifact_path(
    stress_root: &std::path::Path,
    benchmark: &PerformanceBenchmarkScenario,
) -> std::path::PathBuf {
    performance_benchmark_samples::expected_stress_artifact_path(stress_root, benchmark)
}

pub fn summarize_stress_artifact(
    benchmark: &PerformanceBenchmarkScenario,
    artifact_json: &str,
) -> Result<BenchmarkSampleSummary, String> {
    performance_benchmark_samples::summarize_stress_artifact(benchmark, artifact_json)
}

pub fn summarize_stress_artifact_rows(
    artifact_json: &str,
) -> Result<Vec<StressArtifactRowSummary>, String> {
    performance_benchmark_samples::summarize_stress_artifact_rows(artifact_json)
}

pub fn validate_stress_artifact_signal_metadata(artifact_json: &str) -> Result<(), String> {
    performance_benchmark_samples::validate_stress_artifact_signal_metadata(artifact_json)
}
