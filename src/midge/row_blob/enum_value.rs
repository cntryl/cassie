//! CRB1 tag `0x11`: a named enum value (#827).
//!
//! Payload, all integers big-endian: `version: u8 = 1`, `type_oid: u32`,
//! `member_id: u64`. A stored enum value is its immutable member id, never the
//! label or the sort order, so `RENAME VALUE` and `ADD VALUE` rewrite nothing.
//!
//! An enum NULL uses the outer NULL marker. Decoding yields the stored JSON
//! form `{"$cassie_enum_v1": member_id}`; a member id the column's descriptor
//! does not know is corruption (`XX001`, see [`check_column`]).
use crate::app::CassieError;
use crate::types::enum_type::ENUM_STORAGE_KEY;
use crate::types::{DataType, EnumType, EnumValue, SqlArray, Value};

use super::{Cursor, TYPE_ENUM};

pub(super) const ENUM_PAYLOAD_VERSION: u8 = 1;
const PAYLOAD_LEN: usize = 1 + 4 + 8;

/// Encodes a stored enum document against the column's descriptor.
pub(super) fn encode_enum(
    descriptor: &EnumType,
    value: &serde_json::Value,
) -> Result<(u8, Vec<u8>), CassieError> {
    let member_id = EnumValue::storage_member_id(value)
        .filter(|member_id| descriptor.member(*member_id).is_some())
        .ok_or_else(|| {
            CassieError::InvalidVector(format!(
                "enum field expects a member of type {}",
                descriptor.name
            ))
        })?;
    let mut out = Vec::with_capacity(PAYLOAD_LEN);
    out.push(ENUM_PAYLOAD_VERSION);
    out.extend_from_slice(&descriptor.oid.to_be_bytes());
    out.extend_from_slice(&member_id.to_be_bytes());
    Ok((TYPE_ENUM, out))
}

/// Decodes one length-prefixed enum payload from `cursor` into the stored
/// JSON form.
pub(super) fn decode_enum(cursor: &mut Cursor<'_>) -> Result<serde_json::Value, CassieError> {
    let bytes = cursor.read_len_prefixed()?;
    let (_, member_id) = decode_payload(&bytes)?;
    Ok(serde_json::json!({ ENUM_STORAGE_KEY: member_id }))
}

/// Returns the `(type_oid, member_id)` of a payload.
fn decode_payload(bytes: &[u8]) -> Result<(u32, u64), CassieError> {
    let mut payload = Cursor::new(bytes);
    let version = payload.read_u8()?;
    if version != ENUM_PAYLOAD_VERSION {
        return Err(CassieError::Parse(format!(
            "unsupported enum payload version {version}"
        )));
    }
    let oid = payload.read_u32()?;
    let member_id = u64::from_be_bytes(
        payload
            .read_exact(8)?
            .try_into()
            .map_err(|_| CassieError::Parse("invalid enum member id in row blob".to_string()))?,
    );
    if !payload.remaining().is_empty() {
        return Err(CassieError::Parse(
            "enum payload has unconsumed bytes".to_string(),
        ));
    }
    Ok((oid, member_id))
}

fn corrupt(descriptor: &EnumType, detail: &str) -> CassieError {
    CassieError::from(crate::app::DiagnosticError::new(
        "XX001",
        format!(
            "stored enum value does not match type {}: {detail}",
            descriptor.name
        ),
    ))
}

fn check_member(descriptor: &EnumType, value: &serde_json::Value) -> Result<(), CassieError> {
    match EnumValue::storage_member_id(value) {
        Some(member_id) if descriptor.member(member_id).is_some() => Ok(()),
        Some(member_id) => Err(corrupt(
            descriptor,
            &format!("unknown member id {member_id}"),
        )),
        None => Err(corrupt(descriptor, "not an enum document")),
    }
}

fn check_array(descriptor: &EnumType, value: &serde_json::Value) -> Result<(), CassieError> {
    let array =
        SqlArray::from_storage_json(value).map_err(|detail| corrupt(descriptor, &detail))?;
    for element in array.elements() {
        match element {
            Value::Null => {}
            Value::Json(document) => check_member(descriptor, document)?,
            _ => return Err(corrupt(descriptor, "array element is not an enum document")),
        }
    }
    Ok(())
}

/// Verifies that a decoded value of a column typed `data_type` only names
/// members its enum descriptor knows. Other column types pass unchecked.
///
/// # Errors
/// Returns a `XX001` data-corruption error for an unknown member id.
pub(super) fn check_column(
    data_type: &DataType,
    value: &serde_json::Value,
) -> Result<(), CassieError> {
    match data_type {
        DataType::Enum(descriptor) => check_member(descriptor, value),
        DataType::Array(element) => match element.as_ref() {
            DataType::Enum(descriptor) => check_array(descriptor, value),
            _ => Ok(()),
        },
        _ => Ok(()),
    }
}

#[cfg(test)]
#[path = "enum_value_tests.rs"]
mod tests;
