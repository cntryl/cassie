#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum BenchmarkTier {
    Tier1 = 1,
    Tier2 = 2,
    Tier3 = 3,
    Tier4 = 4,
    Tier5 = 5,
    Tier6 = 6,
}

impl BenchmarkTier {
    #[must_use]
    pub const fn number(self) -> u32 {
        self as u32
    }

    #[must_use]
    pub const fn owner_prefix(self) -> &'static str {
        match self {
            Self::Tier1 => "tier1_",
            Self::Tier2 => "tier2_",
            Self::Tier3 => "tier3_",
            Self::Tier4 => "tier4_",
            Self::Tier5 => "tier5_",
            Self::Tier6 => "tier6_",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchmarkTimingMode {
    Micro,
    Measure,
    Counted,
    Batch,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceRole {
    Gate,
    Diagnostic,
}

impl EvidenceRole {
    #[must_use]
    pub const fn signal_role(self) -> &'static str {
        match self {
            Self::Gate => "optimization",
            Self::Diagnostic => "informational",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureClass {
    Kernel,
    Subsystem,
    Representative,
    Integration,
    Scaling,
    Soak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultCachePolicy {
    Disabled,
    Measured,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PerformanceBenchmarkScenario {
    pub scenario_id: &'static str,
    pub family: &'static str,
    pub access_family: &'static str,
    pub benchmark: &'static str,
    pub workload: &'static str,
    pub fixture_scale: &'static str,
    pub fixture_rows: usize,
    pub declared_tier: BenchmarkTier,
    pub timing_mode: BenchmarkTimingMode,
    pub operation_unit: &'static str,
    pub evidence_role: EvidenceRole,
    pub fixture_class: FixtureClass,
    pub result_cache_policy: ResultCachePolicy,
    pub client_count: Option<u16>,
    pub worker_count: Option<u16>,
    pub memory_evidence: &'static str,
    pub fallback_evidence: &'static str,
    pub explain_evidence: &'static str,
    pub metrics_evidence: &'static str,
}

impl PerformanceBenchmarkScenario {
    #[must_use]
    pub fn requires_observed_query_evidence(&self) -> bool {
        matches!(self.evidence_role, EvidenceRole::Gate)
            && matches!(
                self.declared_tier,
                BenchmarkTier::Tier3
                    | BenchmarkTier::Tier4
                    | BenchmarkTier::Tier5
                    | BenchmarkTier::Tier6
            )
            && (self.operation_unit == "query" || self.family == "mixed")
    }

    #[must_use]
    pub fn expected_selected_access_path(&self) -> Option<&'static str> {
        if !self.requires_observed_query_evidence() {
            return None;
        }
        match self.access_family {
            "vector_exact" => Some("vector_exact"),
            "vector_hnsw" => Some("hnsw"),
            "vector_ivf" => Some("ivfflat"),
            _ => None,
        }
    }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StressArtifactRowSummary {
    pub benchmark: String,
    pub workload: String,
    pub fixture_scale: String,
    pub tier: u64,
    pub scenario_id: Option<String>,
    pub family: Option<String>,
    pub signal_role: String,
    pub operation_unit: Option<String>,
    pub logical_operations_per_iteration: Option<u64>,
    pub logical_operations_source: Option<String>,
    pub diagnostic_codes: Vec<String>,
}

impl StressArtifactRowSummary {
    pub fn is_optimization_signal(&self) -> bool {
        self.signal_role != "informational"
    }
}
