use serde::{Deserialize, Serialize};

use super::{Catalog, ProjectionComparisonReportMeta};

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

impl Catalog {
    pub fn register_projection_comparison_report(&self, report: ProjectionComparisonReportMeta) {
        self.projection_comparison_reports
            .write()
            .insert(report.report_id.clone(), report);
        self.bump_version();
    }

    #[must_use]
    pub fn list_projection_comparison_reports(&self) -> Vec<ProjectionComparisonReportMeta> {
        let mut out = self
            .projection_comparison_reports
            .read()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        out.sort_by_key(|report| report.report_id.clone());
        out
    }

    pub fn register_projection_consistency_report(&self, report: ProjectionConsistencyReportMeta) {
        self.projection_consistency_reports
            .write()
            .insert(report.report_id.clone(), report);
        self.bump_version();
    }

    #[must_use]
    pub fn list_projection_consistency_reports(&self) -> Vec<ProjectionConsistencyReportMeta> {
        let mut out = self
            .projection_consistency_reports
            .read()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        out.sort_by_key(|report| report.report_id.clone());
        out
    }

    #[must_use]
    pub fn latest_projection_consistency_reports(&self) -> Vec<ProjectionConsistencyReportMeta> {
        let mut latest =
            std::collections::BTreeMap::<String, ProjectionConsistencyReportMeta>::new();
        for report in self.list_projection_consistency_reports() {
            let replace = latest
                .get(&report.projection_id)
                .is_none_or(|current| current.created_ms <= report.created_ms);
            if replace {
                latest.insert(report.projection_id.clone(), report);
            }
        }
        latest.into_values().collect()
    }
}
