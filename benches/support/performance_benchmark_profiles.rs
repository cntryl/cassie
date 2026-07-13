use super::{DeploymentProfile, PerformanceBenchmarkScenario};

const STANDARD_METRICS_CAPTURED: &[&str] = &[
    "p50_us",
    "p95_us",
    "p99_us",
    "throughput_ops_per_sec",
    "fallback_counters",
    "cache_occupancy",
    "storage_family_operations",
    "feature_family_metrics",
];

const LOCAL_PROFILE_NON_GOALS: &[&str] = &[
    "not_sla",
    "not_ci_gate",
    "not_production_ready_promotion",
    "not_disk_sync_unless_bench_midge_disk",
];

pub const DEPLOYMENT_PROFILES: &[DeploymentProfile] = &[
    DeploymentProfile {
        profile_id: "local-dev-fallback-10k",
        host_shape: "local developer workstation",
        storage_mode: "in_memory_midge_fallback",
        data_shape: "deterministic generated read-model fixture",
        workload_mix: "single benchmark owner workload",
        fixture_scale: "10k",
        benchmark_command: "cargo bench --locked --bench <owner-benchmark>",
        cache_evidence: "plan_cache.entries",
        metrics_captured: STANDARD_METRICS_CAPTURED,
        known_non_goals: LOCAL_PROFILE_NON_GOALS,
        default_manual: true,
    },
    DeploymentProfile {
        profile_id: "local-dev-fallback-100k",
        host_shape: "local developer workstation",
        storage_mode: "in_memory_midge_fallback",
        data_shape: "deterministic generated read-model fixture",
        workload_mix: "single benchmark owner workload",
        fixture_scale: "100k",
        benchmark_command: "cargo bench --locked --bench <owner-benchmark>",
        cache_evidence: "plan_cache.entries",
        metrics_captured: STANDARD_METRICS_CAPTURED,
        known_non_goals: LOCAL_PROFILE_NON_GOALS,
        default_manual: true,
    },
    DeploymentProfile {
        profile_id: "local-dev-fallback-250k",
        host_shape: "local developer workstation",
        storage_mode: "in_memory_midge_fallback",
        data_shape: "deterministic generated read-model fixture",
        workload_mix: "single benchmark owner workload",
        fixture_scale: "250k",
        benchmark_command: "cargo bench --locked --bench <owner-benchmark>",
        cache_evidence: "plan_cache.entries",
        metrics_captured: STANDARD_METRICS_CAPTURED,
        known_non_goals: LOCAL_PROFILE_NON_GOALS,
        default_manual: true,
    },
];

pub fn deployment_profile_for_id(profile_id: &str) -> Option<&'static DeploymentProfile> {
    DEPLOYMENT_PROFILES
        .iter()
        .find(|profile| profile.profile_id == profile_id)
}

pub fn deployment_profile_for_scenario(
    benchmark: &PerformanceBenchmarkScenario,
) -> Option<&'static DeploymentProfile> {
    DEPLOYMENT_PROFILES
        .iter()
        .find(|profile| profile.fixture_scale == benchmark.fixture_scale)
}
