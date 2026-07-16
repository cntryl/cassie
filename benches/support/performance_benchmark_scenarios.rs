use super::PerformanceBenchmarkScenario;

#[path = "performance_benchmark_catalog.rs"]
mod catalog;

pub const SUPPORTED_SCALES: &[&str] = &["10k", "100k", "250k"];

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
    "lifecycle",
    "mixed",
    "protocol",
];

pub fn benchmark_scenarios() -> impl Iterator<Item = &'static PerformanceBenchmarkScenario> {
    catalog::scenarios()
}
