//! CRB1 tag `0x0F`: a named composite value (#828).
//!
//! Payload, all integers big-endian: `version: u8 = 1`, `type_oid: u32`,
//! `descriptor_revision: u64`, `field_count: u32`, then one framed nullable
//! value per descriptor field. Framing matches array elements: a type tag, then
//! the value directly for fixed-width and self-delimiting tags, otherwise a
//! varint length and the bytes. Tag `0x10` stays reserved for domains.
use crate::app::CassieError;
use crate::types::composite::{CompositeType, RECORD_STORAGE_KEY};

use super::encoding::encode_value;
use super::{
    decode_value, write_varint, Cursor, TYPE_ARRAY, TYPE_BOOL, TYPE_COMPOSITE, TYPE_F64, TYPE_I64,
    TYPE_NULL, TYPE_UUID,
};

pub(super) const COMPOSITE_PAYLOAD_VERSION: u8 = 1;
/// Bounds recursion over untrusted nested composite payloads.
const MAX_COMPOSITE_DEPTH: usize = 32;

/// Encodes a record storage document (see
/// [`record_to_storage_json`](crate::types::composite::record_to_storage_json))
/// against the column's descriptor.
pub(super) fn encode_composite(
    descriptor: &CompositeType,
    value: &serde_json::Value,
) -> Result<(u8, Vec<u8>), CassieError> {
    let invalid = || {
        CassieError::InvalidVector(format!(
            "composite field expects a record of type {}",
            descriptor.name
        ))
    };
    let body = value
        .as_object()
        .and_then(|object| object.get(RECORD_STORAGE_KEY))
        .and_then(serde_json::Value::as_object)
        .ok_or_else(invalid)?;
    if body.get("oid").and_then(serde_json::Value::as_u64) != Some(u64::from(descriptor.oid)) {
        return Err(invalid());
    }
    let fields = body
        .get("fields")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(invalid)?;
    if fields.len() != descriptor.physical_len() {
        return Err(invalid());
    }

    let mut out = vec![COMPOSITE_PAYLOAD_VERSION];
    out.extend_from_slice(&descriptor.oid.to_be_bytes());
    out.extend_from_slice(&descriptor.revision.to_be_bytes());
    let count = u32::try_from(fields.len())
        .map_err(|_| CassieError::Parse("composite field count overflow".to_string()))?;
    out.extend_from_slice(&count.to_be_bytes());
    let mut live = descriptor.fields.iter();
    for (slot, stored) in fields.iter().enumerate() {
        // A dropped attribute keeps its slot but never carries a value.
        let field = if descriptor.dropped_positions.contains(&slot) {
            None
        } else {
            live.next()
        };
        let (tag, data) = match field {
            Some(field) if !stored.is_null() => encode_value(&field.data_type, stored)?,
            _ => (TYPE_NULL, Vec::new()),
        };
        out.push(tag);
        match tag {
            TYPE_BOOL | TYPE_I64 | TYPE_F64 | TYPE_UUID | TYPE_ARRAY => {
                out.extend_from_slice(&data);
            }
            _ => {
                write_varint(u64::try_from(data.len()).unwrap_or(u64::MAX), &mut out);
                out.extend_from_slice(&data);
            }
        }
    }
    Ok((TYPE_COMPOSITE, out))
}

/// Verifies that a decoded composite document belongs to `descriptor`: same
/// type identity and no more slots than the type has ever had. Values written
/// under an older revision carry fewer slots and stay valid.
///
/// # Errors
/// Returns a `XX001` data-corruption error when the stored value contradicts
/// its column type.
pub(super) fn check_stored(
    descriptor: &CompositeType,
    value: &serde_json::Value,
) -> Result<(), CassieError> {
    let corrupt = |detail: String| {
        CassieError::from(crate::app::DiagnosticError::new(
            "XX001",
            format!(
                "stored composite value does not match type {}: {detail}",
                descriptor.name
            ),
        ))
    };
    let body = value
        .as_object()
        .and_then(|object| object.get(RECORD_STORAGE_KEY))
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| corrupt("not a record document".to_string()))?;
    let oid = body.get("oid").and_then(serde_json::Value::as_u64);
    if oid != Some(u64::from(descriptor.oid)) {
        return Err(corrupt(format!(
            "type identity {oid:?} differs from {}",
            descriptor.oid
        )));
    }
    let slots = body
        .get("fields")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| corrupt("missing field list".to_string()))?;
    if slots.len() > descriptor.physical_len() {
        return Err(corrupt(format!(
            "{} fields stored but the type has {}",
            slots.len(),
            descriptor.physical_len()
        )));
    }
    for (field, slot) in descriptor
        .fields
        .iter()
        .zip(descriptor.physical_positions())
    {
        if let (crate::types::DataType::Composite(nested), Some(stored)) =
            (&field.data_type, slots.get(slot))
        {
            if !stored.is_null() {
                check_stored(nested, stored)?;
            }
        }
    }
    Ok(())
}

/// Decodes one length-prefixed composite payload from `cursor` into the record
/// storage document.
pub(super) fn decode_composite(
    cursor: &mut Cursor<'_>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<serde_json::Value, CassieError> {
    let bytes = cursor.read_len_prefixed()?;
    decode_payload(&bytes, controls, 0)
}

fn decode_payload(
    bytes: &[u8],
    controls: Option<&crate::runtime::QueryExecutionControls>,
    depth: usize,
) -> Result<serde_json::Value, CassieError> {
    if depth >= MAX_COMPOSITE_DEPTH {
        return Err(CassieError::Parse(
            "composite value nests too deeply in row blob".to_string(),
        ));
    }
    let mut cursor = Cursor::new(bytes);
    let version = cursor.read_u8()?;
    if version != COMPOSITE_PAYLOAD_VERSION {
        return Err(CassieError::Parse(format!(
            "unsupported composite payload version {version}"
        )));
    }
    let oid = cursor.read_u32()?;
    let revision = u64::from_be_bytes(cursor.read_exact(8)?.try_into().map_err(|_| {
        CassieError::Parse("invalid composite descriptor revision in row blob".to_string())
    })?);
    let count = usize::try_from(cursor.read_u32()?)
        .map_err(|_| CassieError::Parse("composite field count out of range".to_string()))?;
    // Every framed field, even NULL, needs at least two bytes.
    if count > cursor.remaining().len() / 2 {
        return Err(CassieError::Parse(
            "composite field count exceeds remaining payload in row blob".to_string(),
        ));
    }
    let mut fields = Vec::with_capacity(count);
    for _ in 0..count {
        let tag = cursor.read_u8()?;
        let field = match tag {
            TYPE_NULL => {
                if cursor.read_varint()? != 0 {
                    return Err(CassieError::Parse(
                        "null composite field carries a nonzero payload length".to_string(),
                    ));
                }
                serde_json::Value::Null
            }
            TYPE_COMPOSITE => {
                let nested = cursor.read_len_prefixed()?;
                decode_payload(&nested, controls, depth + 1)?
            }
            other => decode_value(other, &mut cursor, controls)?,
        };
        fields.push(field);
    }
    if !cursor.remaining().is_empty() {
        return Err(CassieError::Parse(
            "composite payload has unconsumed bytes".to_string(),
        ));
    }
    let mut body = serde_json::Map::new();
    body.insert("oid".to_string(), serde_json::Value::from(oid));
    body.insert("revision".to_string(), serde_json::Value::from(revision));
    body.insert("fields".to_string(), serde_json::Value::Array(fields));
    let mut object = serde_json::Map::new();
    object.insert(
        RECORD_STORAGE_KEY.to_string(),
        serde_json::Value::Object(body),
    );
    Ok(serde_json::Value::Object(object))
}

#[cfg(test)]
#[path = "composite_tests.rs"]
mod tests;
