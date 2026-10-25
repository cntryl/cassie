//! Preserve DEFAULT independently of SQL NULL until column defaults are applied.
use super::{Cassie, DataType, FieldMeta, QueryError, Value};

pub(super) enum InsertValue {
    Default,
    Value(Value),
}

impl InsertValue {
    pub(super) fn as_value(&self) -> Option<&Value> {
        match self {
            Self::Default => None,
            Self::Value(value) => Some(value),
        }
    }
}

pub(super) fn payload(
    cassie: &Cassie,
    table: &str,
    fields: &[FieldMeta],
    row: &[InsertValue],
) -> Result<serde_json::Value, QueryError> {
    let mut object = serde_json::Map::with_capacity(fields.len());
    let mut explicit_json_nulls = Vec::new();
    for (field, cell) in fields.iter().zip(row) {
        if let InsertValue::Value(value) = cell {
            if matches!(field.data_type, DataType::Json | DataType::Jsonb)
                && matches!(value, Value::Null)
            {
                explicit_json_nulls.push(field.name.as_str());
            }
            object.insert(
                field.name.clone(),
                super::value_to_json_for_field(&field.name, value, &field.data_type)?,
            );
        }
    }
    // Explicit NULL keys must remain present while defaults are filled. JSON
    // SQL NULL then uses its existing absent-key storage representation.
    Ok(cassie.apply_insert_defaults(
        table,
        serde_json::Value::Object(object),
        &explicit_json_nulls,
    )?)
}
