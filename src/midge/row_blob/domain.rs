//! CRB1 tag `0x10`: a named domain value (#828).
//!
//! Payload, all integers big-endian: `version: u8 = 1`, `type_oid: u32`,
//! `descriptor_revision: u64`, then one framed base value. Framing matches
//! composite fields: a type tag, then the value directly for fixed-width and
//! self-delimiting tags, otherwise a varint length and the bytes.
//!
//! A domain NULL uses the outer NULL marker, so a NULL base tag inside a
//! domain payload is corrupt: it would let a NULL bypass the domain's NOT NULL
//! constraint. Decoding yields the base value, because a stored document does
//! not carry domain identity.
use crate::app::CassieError;
use crate::types::DomainType;

use super::encoding::encode_value;
use super::{
    decode_value, write_varint, Cursor, TYPE_ARRAY, TYPE_BOOL, TYPE_DOMAIN, TYPE_F64, TYPE_I64,
    TYPE_NULL, TYPE_UUID,
};

pub(super) const DOMAIN_PAYLOAD_VERSION: u8 = 1;

/// Encodes `value` against the domain's base type.
pub(super) fn encode_domain(
    domain: &DomainType,
    value: &serde_json::Value,
) -> Result<(u8, Vec<u8>), CassieError> {
    let (tag, data) = encode_value(&domain.base, value)?;
    if tag == TYPE_NULL {
        return Err(CassieError::Parse(format!(
            "domain {} cannot wrap a NULL base value",
            domain.name
        )));
    }
    let mut out = vec![DOMAIN_PAYLOAD_VERSION];
    out.extend_from_slice(&domain.oid.to_be_bytes());
    out.extend_from_slice(&domain.revision.to_be_bytes());
    out.push(tag);
    match tag {
        TYPE_BOOL | TYPE_I64 | TYPE_F64 | TYPE_UUID | TYPE_ARRAY => out.extend_from_slice(&data),
        _ => {
            write_varint(u64::try_from(data.len()).unwrap_or(u64::MAX), &mut out);
            out.extend_from_slice(&data);
        }
    }
    Ok((TYPE_DOMAIN, out))
}

/// Decodes one length-prefixed domain payload from `cursor` to its base value.
pub(super) fn decode_domain(
    cursor: &mut Cursor<'_>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<serde_json::Value, CassieError> {
    let bytes = cursor.read_len_prefixed()?;
    let mut payload = Cursor::new(&bytes);
    let version = payload.read_u8()?;
    if version != DOMAIN_PAYLOAD_VERSION {
        return Err(CassieError::Parse(format!(
            "unsupported domain payload version {version}"
        )));
    }
    payload.read_u32()?;
    payload.read_exact(8)?;
    let tag = payload.read_u8()?;
    if tag == TYPE_NULL {
        return Err(CassieError::Parse(
            "domain payload wraps a NULL base value".to_string(),
        ));
    }
    let value = decode_value(tag, &mut payload, controls)?;
    if !payload.remaining().is_empty() {
        return Err(CassieError::Parse(
            "domain payload has unconsumed bytes".to_string(),
        ));
    }
    Ok(value)
}

#[cfg(test)]
#[path = "domain_tests.rs"]
mod tests;
