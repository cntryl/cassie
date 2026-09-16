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

    /// Removes repair reports for one projection version and returns their ids.
    #[must_use]
    pub fn remove_projection_repair_reports_for_version(
        &self,
        projection: &str,
        version_id: &str,
    ) -> Vec<String> {
        let mut reports = self.projection_repair_reports.write();
        let ids = reports
            .values()
            .filter(|report| {
                report.projection_name.eq_ignore_ascii_case(projection)
                    && report.version_id.as_deref() == Some(version_id)
            })
            .map(|report| report.report_id.clone())
            .collect::<Vec<_>>();
        for id in &ids {
            reports.remove(id);
        }
        drop(reports);
        if !ids.is_empty() {
            self.bump_version();
        }
        ids
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
