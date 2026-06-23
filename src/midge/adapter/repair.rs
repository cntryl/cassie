use super::*;

impl Midge {
    pub fn put_projection_repair_report(
        &self,
        report: crate::catalog::ProjectionRepairReportMeta,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let value =
            serde_json::to_vec(&report).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(projection_repair_report_key(&report.report_id), value, None)
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

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
