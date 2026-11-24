use super::Catalog;
use crate::types::{DataType, Value};

pub type VirtualRow = Vec<(String, Value)>;

pub fn schema() -> Vec<(String, DataType)> {
    vec![
        text("report_id"),
        text("projection_name"),
        text("target"),
        text("version_id"),
        text("scope"),
        text("action"),
        text("state"),
        bool("executable"),
        text("affected_objects"),
        text("source_report_state"),
        int("source_mismatch_count"),
        int("source_missing_count"),
        int("source_stale_count"),
        text("verification_required"),
        text("post_verification_state"),
        int("created_ms"),
        text("last_error"),
    ]
}

pub fn rows(catalog: &Catalog) -> Vec<VirtualRow> {
    catalog
        .list_projection_repair_reports()
        .into_iter()
        .map(|report| {
            vec![
                string("report_id", report.report_id),
                string("projection_name", report.projection_name),
                string("target", report.target),
                string("version_id", report.version_id.unwrap_or_default()),
                string("scope", report.scope),
                string("action", report.action),
                string("state", report.state),
                bool_value("executable", report.executable),
                string("affected_objects", report.affected_objects.join(",")),
                string("source_report_state", report.source_report_state),
                int_value("source_mismatch_count", report.source_mismatch_count),
                int_value("source_missing_count", report.source_missing_count),
                int_value("source_stale_count", report.source_stale_count),
                string("verification_required", report.verification_required),
                string("post_verification_state", report.post_verification_state),
                int_value("created_ms", report.created_ms),
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

fn bool(name: &str) -> (String, DataType) {
    (name.to_string(), DataType::Boolean)
}

fn string(name: &str, value: String) -> (String, Value) {
    (name.to_string(), Value::String(value))
}

fn int_value<T>(name: &str, value: T) -> (String, Value)
where
    T: TryInto<i64>,
{
    (
        name.to_string(),
        Value::Int64(value.try_into().unwrap_or(i64::MAX)),
    )
}

fn bool_value(name: &str, value: bool) -> (String, Value) {
    (name.to_string(), Value::Bool(value))
}
