//! Compact authoritative-field codecs and shared row-directory decoding.
//!
//! Compact fields enforce the existing declared-type corruption contract before
//! publication. The directory decoder and tag selection remain shared unchanged.

use crate::app::CassieError;
use crate::types::DataType;

#[cfg(test)]
use super::{decode_value, write_varint};
#[cfg(any(test, feature = "transport-test-hooks"))]
use super::{
    Cursor, TYPE_ARRAY, TYPE_BOOL, TYPE_BYTEA, TYPE_COMPOSITE, TYPE_DATE, TYPE_DOMAIN, TYPE_ENUM,
    TYPE_EXTENDED, TYPE_F64, TYPE_I64, TYPE_JSON, TYPE_NULL, TYPE_STRING, TYPE_TIME,
    TYPE_TIMESTAMP, TYPE_UUID, TYPE_VECTOR_F32,
};

pub(crate) fn encode_compact_value(
    data_type: &DataType,
    value: &serde_json::Value,
) -> Result<Vec<u8>, CassieError> {
    cassie_storage::row_blob::encode_compact_value(data_type, value)
        .map_err(super::map_row_value_error)
}

/// Decodes one stored field value against its declared type.
///
/// SQL NULL is tagged `TYPE_NULL`; for JSON columns it decodes to `None`, since
/// JSON `null` has its own tag. Any other tag must be the one the declared type
/// writes, so a flipped tag byte cannot decode as a plausible wrong-type value.
pub(crate) fn decode_compact_field_value(
    data_type: &DataType,
    bytes: &[u8],
) -> Result<Option<serde_json::Value>, CassieError> {
    cassie_storage::row_blob::decode_compact_field_value(
        data_type,
        bytes,
        super::decode_engine_json,
    )
    .map_err(super::map_row_decode_error)
}

/// One stored field value read without building a JSON value for the
/// fixed-width and text types the vector reader keeps in native lanes.
pub(crate) use cassie_storage::row_blob::CompactScalar;

/// Decodes one stored field value like [`decode_compact_field_value`], but
/// keeps integers, floats, Booleans and text out of JSON.
///
/// The tag check, the integer width check and the rejection of a non-finite
/// float are the ones `decode_compact_field_value` applies.
///
/// # Errors
///
/// Returns a parse error when the bytes do not decode as the declared type.
pub(crate) fn decode_compact_scalar<'a>(
    data_type: &DataType,
    bytes: &'a [u8],
) -> Result<CompactScalar<'a>, CassieError> {
    cassie_storage::row_blob::decode_compact_scalar(data_type, bytes, super::decode_engine_json)
        .map_err(super::map_row_decode_error)
}

/// The compact encoding of an explicit SQL NULL, valid for every column type.
pub(crate) fn encode_compact_sql_null() -> Vec<u8> {
    cassie_storage::row_blob::encode_compact_sql_null()
}

#[cfg(test)]
pub(super) fn decode_directory_value(
    type_tag: u8,
    bytes: &[u8],
) -> Result<serde_json::Value, CassieError> {
    let controls = crate::midge::adapter::PublicationControlScope::current_query_controls();
    let mut framed = Vec::new();
    let input = if matches!(
        type_tag,
        TYPE_STRING
            | TYPE_JSON
            | TYPE_VECTOR_F32
            | TYPE_DATE
            | TYPE_TIME
            | TYPE_TIMESTAMP
            | TYPE_BYTEA
            | TYPE_EXTENDED
            | TYPE_COMPOSITE
            | TYPE_DOMAIN
            | TYPE_ENUM
    ) {
        write_varint(u64::try_from(bytes.len()).unwrap_or(u64::MAX), &mut framed);
        framed.extend_from_slice(bytes);
        framed.as_slice()
    } else {
        bytes
    };
    // `decode_array` already checks that its nested elements consume
    // exactly their framed length; apply the same rule at the top level, so
    // a field whose payload decodes a shorter prefix is rejected instead of
    // silently dropping the remainder.
    let mut cursor = Cursor::new(input);
    let value = decode_value(type_tag, &mut cursor, controls.as_ref())?;
    if !cursor.remaining().is_empty() {
        return Err(CassieError::Parse(
            "row blob field payload has unconsumed bytes".to_string(),
        ));
    }
    Ok(value)
}

/// The one type tag `encode_value` writes for a declared field type. Kept
/// beside the decoder so a new `DataType` cannot be added without deciding
/// what its stored tag is.
#[cfg(any(test, feature = "transport-test-hooks"))]
pub(super) fn expected_type_tag(data_type: &DataType) -> u8 {
    match data_type {
        DataType::Null => TYPE_NULL,
        DataType::SmallInt | DataType::Int | DataType::BigInt | DataType::ObjectId(_) => TYPE_I64,
        DataType::Float => TYPE_F64,
        DataType::Boolean => TYPE_BOOL,
        DataType::Name
        | DataType::Text
        | DataType::Char { .. }
        | DataType::Varchar { .. }
        | DataType::JsonPath => TYPE_STRING,
        DataType::Uuid => TYPE_UUID,
        DataType::Date => TYPE_DATE,
        DataType::Time => TYPE_TIME,
        DataType::Timestamp => TYPE_TIMESTAMP,
        DataType::Bytea => TYPE_BYTEA,
        DataType::Array(_) => TYPE_ARRAY,
        DataType::Json | DataType::Jsonb => TYPE_JSON,
        DataType::Vector(_) => TYPE_VECTOR_F32,
        DataType::Extended(_) => TYPE_EXTENDED,
        DataType::Composite(_) => TYPE_COMPOSITE,
        DataType::Domain(_) => TYPE_DOMAIN,
        DataType::Enum(_) => TYPE_ENUM,
    }
}
