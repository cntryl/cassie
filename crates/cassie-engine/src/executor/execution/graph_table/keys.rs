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
    /// The same component in the typed model of the reserved graph types.
    pub(super) fn to_model(&self) -> crate::types::graph::KeyPart {
        use crate::types::graph::KeyPart as Model;
        match self {
            Self::SmallInt(value) => Model::SmallInt(*value),
            Self::Integer(value) => Model::Int(*value),
            Self::BigInt(value) => Model::BigInt(*value),
            Self::Text(value) => Model::Text(value.clone()),
            Self::Uuid(bytes) => Model::Uuid(uuid::Uuid::from_bytes(*bytes)),
        }
    }

    /// Heap bytes this component retains beyond its inline size.
    pub(super) fn heap_bytes(&self) -> usize {
        match self {
            Self::Text(text) => text.capacity(),
            _ => 0,
        }
    }
}

enum BorrowedKeyPart<'a> {
    SmallInt(i16),
    Integer(i32),
    BigInt(i64),
    Text(&'a str),
    Uuid([u8; 16]),
}

impl BorrowedKeyPart<'_> {
    fn into_owned(self) -> KeyPart {
        match self {
            Self::SmallInt(value) => KeyPart::SmallInt(value),
            Self::Integer(value) => KeyPart::Integer(value),
            Self::BigInt(value) => KeyPart::BigInt(value),
            Self::Text(value) => KeyPart::Text(value.to_owned()),
            Self::Uuid(value) => KeyPart::Uuid(value),
        }
    }
}

fn borrowed_key_part<'a>(value: &'a Value, data_type: &DataType) -> Option<BorrowedKeyPart<'a>> {
    match (data_type, value) {
        (DataType::SmallInt, Value::Int64(number)) => {
            i16::try_from(*number).ok().map(BorrowedKeyPart::SmallInt)
        }
        (DataType::Int, Value::Int64(number)) => {
            i32::try_from(*number).ok().map(BorrowedKeyPart::Integer)
        }
        (DataType::BigInt, Value::Int64(number)) => Some(BorrowedKeyPart::BigInt(*number)),
        (DataType::Text, Value::String(text)) => Some(BorrowedKeyPart::Text(text)),
        (DataType::Uuid, Value::String(text)) => uuid::Uuid::parse_str(text)
            .ok()
            .map(|parsed| BorrowedKeyPart::Uuid(*parsed.as_bytes())),
        _ => None,
    }
}

/// The component `value` holds under the declared key type; `None` for NULL or a value the
/// type cannot hold.
pub(super) fn key_part(value: &Value, data_type: &DataType) -> Option<KeyPart> {
    borrowed_key_part(value, data_type).map(BorrowedKeyPart::into_owned)
}

/// The key tuple of one row: every named column typed by the matching declared type. `None`
/// when any component is NULL, which keeps the row from being a graph element or endpoint.
pub(super) fn key_of<'a>(
    mut value_of: impl FnMut(&str) -> Option<&'a Value>,
    columns: &[String],
    types: &[DataType],
) -> Option<Vec<KeyPart>> {
    let mut key = Vec::with_capacity(columns.len());
    for (column, data_type) in columns.iter().zip(types) {
        key.push(key_part(value_of(column)?, data_type)?);
    }
    Some(key)
}

/// Checks the key using the same ordered tuple rules as `key_of` without owning its components.
pub(super) fn key_is_valid<'a>(
    mut value_of: impl FnMut(&str) -> Option<&'a Value>,
    columns: &[String],
    types: &[DataType],
) -> bool {
    columns.iter().zip(types).all(|(column, data_type)| {
        value_of(column).is_some_and(|value| borrowed_key_part(value, data_type).is_some())
    })
}

#[cfg(test)]
#[path = "keys_tests.rs"]
mod tests;
