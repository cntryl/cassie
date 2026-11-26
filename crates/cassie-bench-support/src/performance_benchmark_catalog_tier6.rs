use super::{
    BenchmarkTier, BenchmarkTimingMode, EvidenceRole, FixtureClass, PerformanceBenchmarkScenario,
    ResultCachePolicy,
};

pub const SCENARIOS: &[PerformanceBenchmarkScenario] = &[
    // Tier 6: exactly two bounded one-hour endurance scenarios.
    scenario!(
        "perf.soak.mixed.100k",
        "mixed",
        "mixed_load",
        "tier6_soak_mixed",
        "mixed_query_ingest_retrieval",
        "100k",
        100_000,
        Tier6,
        Batch,
        Soak,
        "operation",
        Disabled,
        None,
        None
    ),
    scenario!(
        "perf.soak.transport_lifecycle.10k",
        "pgwire",
        "transport_lifecycle",
        "tier6_soak_transport",
        "transport_lifecycle",
        "10k",
        10_000,
        Tier6,
        External,
        Soak,
        "operation",
        Disabled,
        None,
        None
    ),
];
