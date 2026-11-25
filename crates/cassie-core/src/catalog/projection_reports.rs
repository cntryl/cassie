//! Persisted projection verification and repair reports.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ProjectionManifestHashMetadata {
    pub algorithm: String,
    pub digest_length: u16,
    pub canonical_encoder_version: u16,
    pub row_hash_version: u16,
    pub range_hash_version: u16,
    pub root_hash_version: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ProjectionManifestRootSummary {
    pub digest: String,
    pub row_count: u64,
    pub range_count: u64,
    pub state: String,
    pub computed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ProjectionManifestRangeSummary {
    pub range_id: u64,
    pub first_row_id: Option<String>,
    pub last_row_id: Option<String>,
    pub row_count: u64,
    pub digest: String,
    pub state: String,
    pub computed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ProjectionManifestRowHashSummary {
    pub row_id: String,
    pub digest: String,
    pub state: String,
    pub computed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ProjectionVerificationManifest {
    pub manifest_version: u16,
    pub instance_id: String,
    pub projection_id: String,
    pub projection_version_id: Option<String>,
    pub projection_kind: String,
    pub schema_epoch: u64,
    pub projection_definition_hash: Option<u64>,
    pub source_identity: Option<String>,
    pub source_checkpoint: Option<String>,
    pub source_position: Option<u64>,
    pub generated_ms: u64,
    pub expires_at_ms: u64,
    pub hash: ProjectionManifestHashMetadata,
    pub root: Option<ProjectionManifestRootSummary>,
    pub ranges: Vec<ProjectionManifestRangeSummary>,
    #[serde(default)]
    pub row_hashes: Vec<ProjectionManifestRowHashSummary>,
    pub manifest_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct ProjectionConsistencyReportMeta {
    pub report_id: String,
    pub created_ms: u64,
    pub projection_id: String,
    pub projection_version_id: Option<String>,
    pub state: String,
    pub compatibility_status: String,
    pub manifest_count: u64,
    pub instance_ids: Vec<String>,
    pub root_digest: Option<String>,
    pub manifest_digest: Option<String>,
    pub mismatch_count: u64,
    pub divergent_range_count: u64,
    pub divergent_row_count: u64,
    pub stale_manifest_count: u64,
    pub incompatible_manifest_count: u64,
    pub unverifiable_count: u64,
    pub diagnostic_sample: Vec<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectionRepairReportMeta {
    pub report_id: String,
    pub created_ms: u64,
    pub projection_name: String,
    pub target: String,
    pub version_id: Option<String>,
    pub scope: String,
    pub action: String,
    pub state: String,
    pub executable: bool,
    pub affected_objects: Vec<String>,
    pub source_report_state: String,
    pub source_mismatch_count: u64,
    pub source_missing_count: u64,
    pub source_stale_count: u64,
    pub verification_required: String,
    pub post_verification_state: String,
    pub last_error: Option<String>,
}
