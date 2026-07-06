use super::PerformanceBenchmarkScenario;

macro_rules! benchmark {
    (
        $scenario_id:literal,
        $family:literal,
        $benchmark:literal,
        $workload:literal,
        $fixture_scale:literal,
        $memory_evidence:literal,
        $fallback_evidence:literal,
        $explain_evidence:literal,
        $metrics_evidence:literal $(,)?
    ) => {
        PerformanceBenchmarkScenario {
            scenario_id: $scenario_id,
            family: $family,
            benchmark: $benchmark,
            workload: $workload,
            fixture_scale: $fixture_scale,
            memory_evidence: $memory_evidence,
            fallback_evidence: $fallback_evidence,
            explain_evidence: $explain_evidence,
            metrics_evidence: $metrics_evidence,
        }
    };
}

#[path = "performance_benchmark_core_scenarios.rs"]
mod core_scenarios;
#[path = "performance_benchmark_rebuild_scenarios.rs"]
mod rebuild_scenarios;
#[path = "performance_benchmark_search_scenarios.rs"]
mod search_scenarios;
#[path = "performance_benchmark_transport_scenarios.rs"]
mod transport_scenarios;

pub const SUPPORTED_SCALES: &[&str] = &["10k", "100k"];

pub const REQUIRED_WORKLOAD_FAMILIES: &[&str] = &[
    "core_read",
    "replay",
    "rebuild",
    "verification",
    "search",
    "vector",
    "hybrid",
    "graph",
    "time_series",
    "pgwire",
    "http",
];

pub fn benchmark_scenarios() -> impl Iterator<Item = &'static PerformanceBenchmarkScenario> {
    core_scenarios::CORE_SCENARIOS
        .iter()
        .chain(rebuild_scenarios::REBUILD_SCENARIOS)
        .chain(search_scenarios::SEARCH_VECTOR_SCENARIOS)
        .chain(transport_scenarios::TRANSPORT_SCENARIOS)
}
