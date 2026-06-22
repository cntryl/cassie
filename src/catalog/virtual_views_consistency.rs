use super::Catalog;
use crate::types::{DataType, Value};

pub type VirtualRow = Vec<(String, Value)>;

pub fn schema() -> Vec<(String, DataType)> {
    vec![
        text("report_id"),
        text("projection_id"),
        text("projection_version_id"),
        text("state"),
        text("compatibility_status"),
        int("manifest_count"),
        text("instance_ids"),
        text("root_digest"),
        text("manifest_digest"),
        int("mismatch_count"),
        int("divergent_range_count"),
        int("divergent_row_count"),
        int("stale_manifest_count"),
        int("incompatible_manifest_count"),
        int("unverifiable_count"),
        text("diagnostic_sample"),
        int("created_ms"),
        text("last_error"),
    ]
}

pub fn rows(catalog: &Catalog) -> Vec<VirtualRow> {
    catalog
        .list_projection_consistency_reports()
        .into_iter()
        .map(|report| {
            vec![
                string("report_id", report.report_id),
                string("projection_id", report.projection_id),
                string(
                    "projection_version_id",
                    report.projection_version_id.unwrap_or_default(),
                ),
                string("state", report.state),
                string("compatibility_status", report.compatibility_status),
                int_value("manifest_count", report.manifest_count as i64),
                string("instance_ids", report.instance_ids.join(",")),
                string("root_digest", report.root_digest.unwrap_or_default()),
                string(
                    "manifest_digest",
                    report.manifest_digest.unwrap_or_default(),
                ),
                int_value("mismatch_count", report.mismatch_count as i64),
                int_value("divergent_range_count", report.divergent_range_count as i64),
                int_value("divergent_row_count", report.divergent_row_count as i64),
                int_value("stale_manifest_count", report.stale_manifest_count as i64),
                int_value(
                    "incompatible_manifest_count",
                    report.incompatible_manifest_count as i64,
                ),
                int_value("unverifiable_count", report.unverifiable_count as i64),
                string("diagnostic_sample", report.diagnostic_sample.join(",")),
                int_value("created_ms", report.created_ms as i64),
                string("last_error", report.last_error.unwrap_or_default()),
            ]
        })
        .collect()
}

fn text(name: &str) -> (String, DataType) {
    (name.to_string(), DataType::Text)
}

fn int(name: &str) -> (String, DataType) {
    (name.to_string(), DataType::Int)
}

fn string(name: &str, value: String) -> (String, Value) {
    (name.to_string(), Value::String(value))
}

fn int_value(name: &str, value: i64) -> (String, Value) {
    (name.to_string(), Value::Int64(value))
}
