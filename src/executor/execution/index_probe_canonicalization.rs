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

/// Whether converting a SQL comparison probe to its stored key shape is exact.
/// TEXT timestamp shapes use instant comparison; FLOAT probes must not round an integer.
pub(super) fn probe_comparison_is_exact(
    cassie: &Cassie,
    collection: &str,
    field: &str,
    value: &serde_json::Value,
) -> bool {
    match cassie.catalog.field_type(collection, field) {
        Some(
            crate::types::DataType::Text
            | crate::types::DataType::Char { .. }
            | crate::types::DataType::Varchar { .. },
        ) => !value
            .as_str()
            .is_some_and(crate::types::temporal::is_canonical_timestamp_text),
        Some(crate::types::DataType::Float) => value.as_i64().is_none_or(|integer| {
            value.as_f64().is_some_and(|float| {
                crate::types::semantic::compare_numeric_values(
                    &crate::types::Value::Int64(integer),
                    &crate::types::Value::Float64(float),
                ) == Some(std::cmp::Ordering::Equal)
            })
        }),
        _ => true,
    }
}
