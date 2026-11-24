use super::{Catalog, ProjectionComparisonReportMeta};

pub use cassie_core::catalog::projection_reports::{
    ProjectionConsistencyReportMeta, ProjectionManifestHashMetadata,
    ProjectionManifestRangeSummary, ProjectionManifestRootSummary,
    ProjectionManifestRowHashSummary, ProjectionVerificationManifest,
};

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

    /// Returns the ids of comparison reports recorded for one projection version.
    #[must_use]
    pub fn projection_comparison_report_ids_for_version(
        &self,
        projection: &str,
        version_id: &str,
    ) -> Vec<String> {
        self.projection_comparison_reports
            .read()
            .values()
            .filter(|report| {
                report.target.eq_ignore_ascii_case(projection)
                    && report.target_version_id.as_deref() == Some(version_id)
            })
            .map(|report| report.report_id.clone())
            .collect()
    }

    /// Returns the ids of every comparison report recorded for one projection.
    #[must_use]
    pub fn projection_comparison_report_ids(&self, projection: &str) -> Vec<String> {
        let mut ids = self
            .projection_comparison_reports
            .read()
            .values()
            .filter(|report| report.target.eq_ignore_ascii_case(projection))
            .map(|report| report.report_id.clone())
            .collect::<Vec<_>>();
        ids.sort();
        ids
    }

    /// Removes comparison reports by id from the catalog.
    pub fn unregister_projection_comparison_reports(&self, report_ids: &[String]) {
        let mut reports = self.projection_comparison_reports.write();
        let removed = report_ids
            .iter()
            .filter(|id| reports.remove(id.as_str()).is_some())
            .count();
        drop(reports);
        if removed > 0 {
            self.bump_version();
        }
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
