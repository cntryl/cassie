use serde::{Deserialize, Serialize};

use super::Catalog;

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

impl Catalog {
    pub fn register_projection_repair_report(&self, report: ProjectionRepairReportMeta) {
        self.projection_repair_reports
            .write()
            .insert(report.report_id.clone(), report);
        self.bump_version();
    }

    #[must_use]
    pub fn list_projection_repair_reports(&self) -> Vec<ProjectionRepairReportMeta> {
        let mut out = self
            .projection_repair_reports
            .read()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        out.sort_by_key(|report| report.report_id.clone());
        out
    }
}
