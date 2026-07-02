#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PerformanceBenchmarkScenario {
    pub scenario_id: &'static str,
    pub family: &'static str,
    pub benchmark: &'static str,
    pub workload: &'static str,
    pub fixture_scale: &'static str,
    pub memory_evidence: &'static str,
    pub fallback_evidence: &'static str,
    pub explain_evidence: &'static str,
    pub metrics_evidence: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeploymentProfile {
    pub profile_id: &'static str,
    pub host_shape: &'static str,
    pub storage_mode: &'static str,
    pub data_shape: &'static str,
    pub workload_mix: &'static str,
    pub fixture_scale: &'static str,
    pub benchmark_command: &'static str,
    pub cache_evidence: &'static str,
    pub metrics_captured: &'static [&'static str],
    pub known_non_goals: &'static [&'static str],
    pub default_manual: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BenchmarkSampleSummary {
    pub profile_id: &'static str,
    pub scenario_id: &'static str,
    pub benchmark: &'static str,
    pub workload: &'static str,
    pub fixture_scale: &'static str,
    pub storage_mode: &'static str,
    pub storage_evidence: &'static str,
    pub fallback_evidence: &'static str,
    pub cache_evidence: &'static str,
    pub feature_evidence: &'static str,
    pub known_non_goals: &'static [&'static str],
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub throughput_ops_per_sec: f64,
}
