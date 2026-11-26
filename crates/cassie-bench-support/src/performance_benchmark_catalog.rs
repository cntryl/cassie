use super::super::{
    BenchmarkTier, BenchmarkTimingMode, EvidenceRole, FixtureClass, PerformanceBenchmarkScenario,
    ResultCachePolicy,
};

macro_rules! scenario {
    (
        $id:literal, $family:literal, $access:literal,
        $owner:literal, $workload:literal, $scale:literal, $rows:expr,
        $tier:ident, $timing:ident, $class:ident, $unit:literal,
        $cache:ident, $clients:expr, $workers:expr $(,)?
    ) => {
        PerformanceBenchmarkScenario {
            scenario_id: $id,
            family: $family,
            access_family: $access,
            benchmark: $owner,
            workload: $workload,
            fixture_scale: $scale,
            fixture_rows: $rows,
            declared_tier: BenchmarkTier::$tier,
            timing_mode: BenchmarkTimingMode::$timing,
            operation_unit: $unit,
            evidence_role: EvidenceRole::Gate,
            fixture_class: FixtureClass::$class,
            result_cache_policy: ResultCachePolicy::$cache,
            client_count: $clients,
            worker_count: $workers,
            memory_evidence: "recorded_peak_query_memory_bytes",
            fallback_evidence: "recorded_fallback_reason",
            explain_evidence: $access,
            metrics_evidence: "recorded_runtime_metrics",
        }
    };
}

#[path = "performance_benchmark_catalog_tier1.rs"]
mod tier1;
#[path = "performance_benchmark_catalog_tier2.rs"]
mod tier2;
#[path = "performance_benchmark_catalog_tier3.rs"]
mod tier3;
#[path = "performance_benchmark_catalog_tier4.rs"]
mod tier4;
#[path = "performance_benchmark_catalog_tier5.rs"]
mod tier5;
#[path = "performance_benchmark_catalog_tier6.rs"]
mod tier6;

pub fn scenarios() -> impl Iterator<Item = &'static PerformanceBenchmarkScenario> {
    tier1::SCENARIOS
        .iter()
        .chain(tier2::SCENARIOS)
        .chain(tier3::SCENARIOS)
        .chain(tier4::SCENARIOS)
        .chain(tier5::SCENARIOS)
        .chain(tier5::LEGACY_SCENARIOS)
        .chain(tier6::SCENARIOS)
}
