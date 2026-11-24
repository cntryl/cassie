//! Portable projection freshness, hash, verification, version, and report records.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionRebuildState {
    Idle,
    Rebuilding,
    Failed,
}

impl ProjectionRebuildState {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Rebuilding => "rebuilding",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionFreshness {
    #[default]
    Unknown,
    Fresh,
    Stale,
    Rebuilding,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionVerificationState {
    #[default]
    Unknown,
    Current,
    Stale,
    Missing,
    Incomplete,
    Incompatible,
    Empty,
    Pending,
    Running,
    Verified,
    Failed,
    Unverifiable,
    Skipped,
}

impl ProjectionVerificationState {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Current => "current",
            Self::Stale => "stale",
            Self::Missing => "missing",
            Self::Incomplete => "incomplete",
            Self::Incompatible => "incompatible",
            Self::Empty => "empty",
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Verified => "verified",
            Self::Failed => "failed",
            Self::Unverifiable => "unverifiable",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionHashAlgorithmMeta {
    pub algorithm: String,
    pub digest_length: u16,
    pub canonical_encoder_version: u16,
    pub hash_version: u16,
}

impl Default for ProjectionHashAlgorithmMeta {
    fn default() -> Self {
        Self {
            algorithm: "cassie-fnv128".to_string(),
            digest_length: 16,
            canonical_encoder_version: 1,
            hash_version: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectionHashCoverageMeta {
    pub state: ProjectionVerificationState,
    pub row_count: u64,
    pub range_count: u64,
    pub source_checkpoint: Option<String>,
    pub projection_version_id: Option<String>,
    pub last_computed_ms: Option<u64>,
    pub digest: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectionHashMeta {
    #[serde(default)]
    pub algorithm: ProjectionHashAlgorithmMeta,
    #[serde(default)]
    pub rows: ProjectionHashCoverageMeta,
    #[serde(default)]
    pub ranges: ProjectionHashCoverageMeta,
    #[serde(default)]
    pub root: ProjectionHashCoverageMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectionRebuildVerificationMeta {
    pub state: ProjectionVerificationState,
    pub started_ms: Option<u64>,
    pub completed_ms: Option<u64>,
    pub mismatch_count: u64,
    pub unverifiable_ranges: u64,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectionIntegrityReportMeta {
    pub state: ProjectionVerificationState,
    pub target: Option<String>,
    pub version_id: Option<String>,
    pub mode: String,
    pub checked_components: Vec<String>,
    pub skipped_components: Vec<String>,
    pub mismatch_count: u64,
    pub missing_count: u64,
    pub stale_count: u64,
    pub repairable: bool,
    pub elapsed_ms: u64,
    pub completed_ms: Option<u64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectionComparisonReportMeta {
    pub report_id: String,
    pub created_ms: u64,
    pub target: String,
    pub target_version_id: Option<String>,
    pub state: String,
    pub compatibility_status: String,
    pub root_digest: Option<String>,
    pub manifest_digest: Option<String>,
    pub mismatch_count: u64,
    pub unverifiable_count: u64,
    pub diagnostic_sample: Vec<String>,
    pub last_error: Option<String>,
}

impl ProjectionFreshness {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Fresh => "fresh",
            Self::Stale => "stale",
            Self::Rebuilding => "rebuilding",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionKind {
    #[default]
    Collection,
    Materialized,
}

impl ProjectionKind {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Collection => "collection",
            Self::Materialized => "materialized",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterializedProjectionState {
    Building,
    Ready,
    Stale,
    Failed,
}

impl MaterializedProjectionState {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Building => "building",
            Self::Ready => "ready",
            Self::Stale => "stale",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionVersionState {
    Building,
    Built,
    Active,
    Failed,
    Retired,
}

impl ProjectionVersionState {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Building => "building",
            Self::Built => "built",
            Self::Active => "active",
            Self::Failed => "failed",
            Self::Retired => "retired",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionVersionMeta {
    pub version_id: String,
    pub output_collection: String,
    pub definition_fingerprint: u64,
    pub source_schema_epoch: u64,
    pub state: ProjectionVersionState,
    pub created_ms: u64,
    pub activated_ms: Option<u64>,
    pub retired_ms: Option<u64>,
    pub last_error: Option<String>,
    #[serde(default)]
    pub verification: ProjectionRebuildVerificationMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectionSwapMeta {
    pub target_version_id: Option<String>,
    pub previous_version_id: Option<String>,
    pub swapped_at_ms: Option<u64>,
    pub unsafe_override: bool,
    pub last_error: Option<String>,
}

