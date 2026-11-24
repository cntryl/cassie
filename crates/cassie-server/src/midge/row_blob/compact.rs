//! Compact authoritative-field codecs and shared row-directory decoding.
//!
//! Compact fields enforce the existing declared-type corruption contract before
//! publication. The directory decoder and tag selection remain shared unchanged.

use crate::app::CassieError;
use crate::types::DataType;

use super::{
    composite, decode_value, encode_value, enum_value, write_varint, Cursor, TYPE_ARRAY, TYPE_BOOL,
    TYPE_BYTEA, TYPE_COMPOSITE, TYPE_DATE, TYPE_DOMAIN, TYPE_ENUM, TYPE_EXTENDED, TYPE_F64,
    TYPE_I64, TYPE_JSON, TYPE_NULL, TYPE_STRING, TYPE_TIME, TYPE_TIMESTAMP, TYPE_UUID,
    TYPE_VECTOR_F32,
};

pub(crate) fn encode_compact_value(
    data_type: &DataType,
    value: &serde_json::Value,
) -> Result<Vec<u8>, CassieError> {
    let (type_tag, payload) = encode_value(data_type, value)?;
    let mut out = Vec::with_capacity(payload.len().saturating_add(1));
    out.push(type_tag);
    out.extend_from_slice(&payload);
    Ok(out)
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
    let (&type_tag, payload) = bytes
        .split_first()
        .ok_or_else(|| CassieError::Parse("empty compact value".to_string()))?;
    if type_tag == TYPE_NULL {
        let decoded = decode_directory_value(type_tag, payload)?;
        return Ok((!matches!(data_type, DataType::Json | DataType::Jsonb)).then_some(decoded));
    }
    let expected = expected_type_tag(data_type);
    if type_tag != expected {
        return Err(CassieError::Parse(format!(
            "stored type tag {type_tag} but the declared type uses type tag {expected}"
        )));
    }
    check_extended_identity(data_type, payload)?;
    let decoded = decode_directory_value(type_tag, payload)?;
    check_integer_width(data_type, &decoded)?;
    if let DataType::Composite(descriptor) = data_type {
        composite::check_stored(descriptor, &decoded)?;
    }
    enum_value::check_column(data_type, &decoded)?;
    Ok(Some(decoded))
}

/// One stored field value read without building a JSON value for the
/// fixed-width and text types the vector reader keeps in native lanes.
#[derive(Debug, PartialEq)]
pub(crate) enum CompactScalar<'a> {
    /// SQL NULL, or a JSON column's absent value.
    Null,
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Text(&'a str),
    /// Every other type, exactly as `decode_compact_field_value` returns it.
    Generic(Option<serde_json::Value>),
}

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
    let native = matches!(
        data_type,
        DataType::SmallInt
            | DataType::Int
            | DataType::BigInt
            | DataType::Float
            | DataType::Boolean
            | DataType::Text
            | DataType::Varchar { .. }
            | DataType::Char { .. }
    );
    if !native {
        return decode_compact_field_value(data_type, bytes).map(CompactScalar::Generic);
    }
    let (&type_tag, payload) = bytes
        .split_first()
        .ok_or_else(|| CassieError::Parse("empty compact value".to_string()))?;
    if type_tag == TYPE_NULL {
        decode_directory_value(type_tag, payload)?;
        return Ok(CompactScalar::Null);
    }
    let expected = expected_type_tag(data_type);
    if type_tag != expected {
        return Err(CassieError::Parse(format!(
            "stored type tag {type_tag} but the declared type uses type tag {expected}"
        )));
    }
    let eight = |what: &str| {
        <[u8; 8]>::try_from(payload).map_err(|_| {
            CassieError::Parse(if payload.len() > 8 {
                "row blob field payload has unconsumed bytes".to_string()
            } else {
                format!("invalid {what} in row blob")
            })
        })
    };
    match data_type {
        DataType::Float => {
            let value = f64::from_be_bytes(eight("f64")?);
            if value.is_finite() {
                Ok(CompactScalar::Float(value))
            } else {
                Err(CassieError::Parse("invalid f64 in row blob".to_string()))
            }
        }
        DataType::Boolean => match payload {
            [flag] => Ok(CompactScalar::Boolean(*flag != 0)),
            [] => Err(CassieError::Parse("invalid bool in row blob".to_string())),
            _ => Err(CassieError::Parse(
                "row blob field payload has unconsumed bytes".to_string(),
            )),
        },
        DataType::Text | DataType::Varchar { .. } | DataType::Char { .. } => {
            std::str::from_utf8(payload)
                .map(CompactScalar::Text)
                .map_err(|error| CassieError::Parse(format!("invalid UTF-8 in row blob: {error}")))
        }
        _ => {
            let value = i64::from_be_bytes(eight("i64")?);
            let fits = match data_type {
                DataType::SmallInt => i16::try_from(value).is_ok(),
                DataType::Int => i32::try_from(value).is_ok(),
                _ => true,
            };
            if fits {
                Ok(CompactScalar::Integer(value))
            } else {
                Err(CassieError::Parse(format!(
                    "stored integer is outside declared {} range",
                    data_type.type_name()
                )))
            }
        }
    }
}

/// The compact encoding of an explicit SQL NULL, valid for every column type.
pub(crate) fn encode_compact_sql_null() -> Vec<u8> {
    vec![TYPE_NULL]
}

fn check_extended_identity(data_type: &DataType, payload: &[u8]) -> Result<(), CassieError> {
    let DataType::Extended(kind) = data_type else {
        return Ok(());
    };
    let oid = payload
        .first_chunk::<4>()
        .ok_or_else(|| CassieError::Parse("truncated compact extended type OID".to_owned()))?;
    let stored_oid = u32::from_be_bytes(*oid);
    let expected_oid = kind.type_oid();
    if stored_oid != expected_oid {
        return Err(CassieError::Parse(format!(
            "stored type OID {stored_oid} but the declared type uses OID {expected_oid}"
        )));
    }
    Ok(())
}

fn check_integer_width(
    data_type: &DataType,
    decoded: &serde_json::Value,
) -> Result<(), CassieError> {
    let valid = match data_type {
        DataType::SmallInt => decoded
            .as_i64()
            .is_some_and(|value| i16::try_from(value).is_ok()),
        DataType::Int => decoded
            .as_i64()
            .is_some_and(|value| i32::try_from(value).is_ok()),
        DataType::ObjectId(_) => decoded
            .as_i64()
            .is_some_and(|value| u32::try_from(value).is_ok()),
        _ => true,
    };
    if !valid {
        return Err(CassieError::Parse(format!(
            "stored integer is outside declared {} range",
            data_type.type_name()
        )));
    }
    Ok(())
}

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
