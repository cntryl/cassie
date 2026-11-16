//! Typed key components of mapped graph elements.

use crate::types::{DataType, Value};

/// One typed key component. Variant order is the contract's type order (smallint, integer,
/// bigint, text, uuid), so the derived `Ord` compares the declared type and then the value
/// (text in C byte order, UUID as unsigned 16 bytes).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum KeyPart {
    SmallInt(i16),
    Integer(i32),
    BigInt(i64),
    Text(String),
    Uuid([u8; 16]),
}

impl KeyPart {
    /// Heap bytes this component retains beyond its inline size.
    pub(super) fn heap_bytes(&self) -> usize {
        match self {
            Self::Text(text) => text.capacity(),
            _ => 0,
        }
    }
}

/// The component `value` holds under the declared key type; `None` for NULL or a value the
/// type cannot hold.
pub(super) fn key_part(value: &Value, data_type: &DataType) -> Option<KeyPart> {
    match (data_type, value) {
        (DataType::SmallInt, Value::Int64(number)) => {
            i16::try_from(*number).ok().map(KeyPart::SmallInt)
        }
        (DataType::Int, Value::Int64(number)) => i32::try_from(*number).ok().map(KeyPart::Integer),
        (DataType::BigInt, Value::Int64(number)) => Some(KeyPart::BigInt(*number)),
        (DataType::Text, Value::String(text)) => Some(KeyPart::Text(text.clone())),
        (DataType::Uuid, Value::String(text)) => uuid::Uuid::parse_str(text)
            .ok()
            .map(|parsed| KeyPart::Uuid(*parsed.as_bytes())),
        _ => None,
    }
}

/// The key tuple of one row: every named column typed by the matching declared type. `None`
/// when any component is NULL, which keeps the row from being a graph element or endpoint.
pub(super) fn key_of<'a>(
    mut value_of: impl FnMut(&str) -> Option<&'a Value>,
    columns: &[String],
    types: &[DataType],
) -> Option<Vec<KeyPart>> {
    columns
        .iter()
        .zip(types)
        .map(|(column, data_type)| key_part(value_of(column)?, data_type))
        .collect()
}
