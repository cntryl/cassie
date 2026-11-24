use std::cell::Cell;

use super::{key_encoding, CassieError, Midge, StorageFamily};

thread_local! {
    static PROJECTION_REPORT_DELETION_FAILPOINT: Cell<bool> = const { Cell::new(false) };
}

/// Fails the next stored projection report deletion on this thread.
#[doc(hidden)]
pub fn set_projection_report_deletion_failure_point(enabled: bool) {
    PROJECTION_REPORT_DELETION_FAILPOINT.set(enabled);
}

fn check_projection_report_deletion_failure_point() -> Result<(), CassieError> {
    if PROJECTION_REPORT_DELETION_FAILPOINT.replace(false) {
        return Err(CassieError::Execution(
            "injected projection report deletion failure".to_string(),
        ));
    }
    Ok(())
}

impl Midge {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_projection_repair_report(
        &self,
        report: &crate::catalog::ProjectionRepairReportMeta,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let value =
            serde_json::to_vec(report).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(projection_repair_report_key(&report.report_id), value, None)
            .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// Deletes stored repair and comparison reports by id in one transaction.
    ///
    /// # Errors
    ///
    /// Returns an error when the storage transaction fails.
    pub fn delete_projection_reports(
        &self,
        repair_report_ids: &[String],
        comparison_report_ids: &[String],
    ) -> Result<(), CassieError> {
        check_projection_report_deletion_failure_point()?;
        if repair_report_ids.is_empty() && comparison_report_ids.is_empty() {
            return Ok(());
        }
        let mut tx = self.begin_schema_rw_tx()?;
        for id in repair_report_ids {
            tx.delete(projection_repair_report_key(id))
                .map_err(CassieError::from)?;
        }
        for id in comparison_report_ids {
            tx.delete(key_encoding::projection_comparison_report_key(id))
                .map_err(CassieError::from)?;
        }
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn list_projection_repair_reports(
        &self,
    ) -> Result<Vec<crate::catalog::ProjectionRepairReportMeta>, CassieError> {
        let entries = self.raw_scan_prefix(
            StorageFamily::Schema,
            projection_repair_report_prefix().as_slice(),
        )?;
        let mut out = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(report) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(report);
        }
        out.sort_by_key(|report: &crate::catalog::ProjectionRepairReportMeta| {
            report.report_id.clone()
        });
        Ok(out)
    }
}

fn projection_repair_report_key(report_id: &str) -> Vec<u8> {
    key_encoding::projection_repair_report_key(report_id)
}

fn projection_repair_report_prefix() -> Vec<u8> {
    key_encoding::projection_repair_report_prefix()
}
