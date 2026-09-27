use super::Cassie;

/// Canonicalizes a field probe the same way scalar-index keys are stored.
pub(super) fn canonicalize_index_probe_value(
    cassie: &Cassie,
    collection: &str,
    field: &str,
    value: &mut serde_json::Value,
) {
    match cassie.catalog.field_type(collection, field) {
        Some(crate::types::DataType::Float) => {
            super::index_read::canonicalize_float_number(value);
        }
        Some(crate::types::DataType::Date) => {
            canonicalize_temporal_text(value, crate::types::temporal::canonical_date);
        }
        Some(crate::types::DataType::Timestamp) => {
            canonicalize_temporal_text(value, crate::types::temporal::canonical_timestamp);
        }
        _ => {}
    }
}

fn canonicalize_temporal_text(
    value: &mut serde_json::Value,
    canonicalize: fn(&str) -> Result<String, String>,
) {
    let Some(text) = value.as_str() else {
        return;
    };
    if let Ok(canonical) = canonicalize(text) {
        *value = serde_json::Value::String(canonical);
    }
}
