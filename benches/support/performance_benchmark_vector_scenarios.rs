use super::PerformanceBenchmarkScenario;

pub const VECTOR_PATH_SCENARIOS: &[PerformanceBenchmarkScenario] = &[
    PerformanceBenchmarkScenario {
        scenario_id: "perf.vector.bruteforce_candidates.10k",
        family: "vector",
        benchmark: "tier2_subsystem_vector",
        workload: "vector_bruteforce_candidates",
        fixture_scale: "10k",
        memory_evidence: "vector.candidate_count_total",
        fallback_evidence: "vector.normalized_fallback_count_total",
        explain_evidence: "access_path=vector_bruteforce",
        metrics_evidence: "vector.latency_ms_total",
    },
    PerformanceBenchmarkScenario {
        scenario_id: "perf.vector.hnsw_candidates.10k",
        family: "vector",
        benchmark: "tier2_subsystem_vector",
        workload: "vector_hnsw_candidates",
        fixture_scale: "10k",
        memory_evidence: "vector.hnsw_candidate_count_total",
        fallback_evidence: "vector.hnsw_fallback_reason",
        explain_evidence: "access_path=hnsw",
        metrics_evidence: "vector.latency_ms_total",
    },
    PerformanceBenchmarkScenario {
        scenario_id: "perf.vector.ivfflat_probe_lists.10k",
        family: "vector",
        benchmark: "tier2_subsystem_vector",
        workload: "vector_ivfflat_probe_lists",
        fixture_scale: "10k",
        memory_evidence: "vector.ivfflat_probe_lists",
        fallback_evidence: "vector.ivfflat_fallback_reason",
        explain_evidence: "access_path=ivfflat",
        metrics_evidence: "vector.latency_ms_total",
    },
];
