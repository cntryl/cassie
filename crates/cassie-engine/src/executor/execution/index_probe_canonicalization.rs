use super::Cassie;

/// Canonicalizes a field probe the same way scalar-index keys are stored.
/// The column type of a probe. `numeric` and `real` columns are also found by
/// their exact stored name, so a dotted quoted column name still gets key text.
pub(super) fn probe_field_type(
    cassie: &Cassie,
    collection: &str,
    field: &str,
) -> Option<crate::types::DataType> {
    use crate::types::extended::ExtendedType;
    cassie.catalog.field_type(collection, field).or_else(|| {
        cassie
            .catalog
            .get_schema(collection)?
            .fields
            .into_iter()
            .find(|entry| entry.name == field)
            .map(|entry| entry.data_type)
            .filter(|data_type| {
                matches!(
                    data_type,
                    crate::types::DataType::Extended(ExtendedType::Numeric(_) | ExtendedType::Real)
                )
            })
    })
}

pub(super) fn canonicalize_index_probe_value(
    cassie: &Cassie,
    collection: &str,
    field: &str,
    value: &mut serde_json::Value,
) {
    match probe_field_type(cassie, collection, field) {
        Some(crate::types::DataType::Float) => {
            super::index_read::canonicalize_float_number(value);
        }
        Some(crate::types::DataType::Extended(crate::types::extended::ExtendedType::Numeric(
            _,
        ))) => {
            if let Some(key) = crate::types::extended::index_key::numeric_index_carrier(value) {
                *value = key;
            }
        }
        Some(crate::types::DataType::Extended(crate::types::extended::ExtendedType::Real)) => {
            if let Some(key) = crate::types::extended::index_key::real_probe_index_carrier(value) {
                *value = key;
            }
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
    match probe_field_type(cassie, collection, field) {
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
        // An enum key is its member id: only a stored-form probe is exact; a label
        // is resolved by the SQL filter. Enum arrays always use the SQL filter.
        Some(crate::types::DataType::Enum(_)) => {
            crate::types::EnumValue::storage_member_id(value).is_some()
        }
        Some(crate::types::DataType::Extended(crate::types::extended::ExtendedType::Numeric(
            _,
        ))) => crate::types::extended::index_key::numeric_probe_is_lossless(value),
        Some(crate::types::DataType::Extended(crate::types::extended::ExtendedType::Real)) => {
            crate::types::extended::index_key::real_probe_index_carrier(value).is_some()
        }
        Some(data_type) => key_order_matches_value_order(&data_type),
        // A name `field_type` cannot resolve (a dotted quoted column) keeps its
        // legacy exact probe; `probe_field_type` resolves the types that need keys.
        None => true,
    }
}

/// Types whose stored key order and equality are their value order and
/// equality, so an index probe reaches exactly the matching rows. A type that
/// is not listed here is not assumed exact: its probes fall back to the SQL
/// filter until its key encoding is proven order-preserving.
fn key_order_matches_value_order(data_type: &crate::types::DataType) -> bool {
    use crate::types::DataType;
    matches!(
        data_type,
        DataType::SmallInt
            | DataType::Int
            | DataType::BigInt
            | DataType::Boolean
            | DataType::Name
            | DataType::Uuid
            | DataType::Bytea
            | DataType::Date
            | DataType::Time
            | DataType::Timestamp
            | DataType::Json
            | DataType::Jsonb
    ) || matches!(data_type, DataType::Domain(domain) if key_order_matches_value_order(&domain.base))
}

/// Types whose index key byte order is the SQL sort order, so an ordered index
/// scan may stand in for `ORDER BY`. Anything not listed is sorted by SQL.
pub(super) fn index_key_order_is_sort_order(data_type: Option<&crate::types::DataType>) -> bool {
    use crate::types::extended::ExtendedType;
    use crate::types::DataType;
    matches!(
        data_type,
        Some(
            DataType::SmallInt
                | DataType::Int
                | DataType::BigInt
                | DataType::Float
                | DataType::Boolean
                | DataType::Uuid
                | DataType::Bytea
                | DataType::Date
                | DataType::Time
                | DataType::Timestamp
                | DataType::Extended(ExtendedType::Numeric(_) | ExtendedType::Real)
        )
    )
}
