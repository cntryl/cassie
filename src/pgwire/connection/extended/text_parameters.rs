//! Text-format (format code 0) bind parameter decoding.
//!
//! A bind parameter must decode to the same `Value` whether the client sent
//! it in text or binary format, so every declared OID with a binary codec
//! canonicalizes its text form here the way `binary_to_value` does, and
//! accepts exactly the spellings the PostgreSQL input function accepts.

use std::str;

use super::super::codecs::{decode_bytea, hex_bytea};
use super::ExtendedQueryError;
use crate::types::temporal::{canonical_date, canonical_time, canonical_timestamp};
use crate::types::{DataType, Value};

const OID_BOOL: i32 = 16;
const OID_BYTEA: i32 = 17;
const OID_INT8: i32 = 20;
const OID_INT2: i32 = 21;
const OID_INT4: i32 = 23;
const OID_JSON: i32 = 114;
const OID_FLOAT4: i32 = 700;
const OID_FLOAT8: i32 = 701;
const OID_DATE: i32 = 1082;
const OID_TIME: i32 = 1083;
const OID_TIMESTAMP: i32 = 1114;
const OID_NUMERIC: i32 = 1700;
const OID_UUID: i32 = 2950;

pub(super) fn decode_text_parameter(
    parameter: &[u8],
    oid: i32,
) -> Result<Value, ExtendedQueryError> {
    let text = str::from_utf8(parameter)
        .map_err(|_| ExtendedQueryError::protocol("bind parameter is not valid UTF-8"))?;
    match oid {
        OID_BOOL => parse_bool(text).map(Value::Bool),
        OID_INT2 => parse_integer(text, i64::from(i16::MIN), i64::from(i16::MAX)),
        OID_INT4 => parse_integer(text, i64::from(i32::MIN), i64::from(i32::MAX)),
        OID_INT8 => parse_integer(text, i64::MIN, i64::MAX),
        OID_FLOAT4 | OID_FLOAT8 => parse_float(text).map(Value::Float64),
        OID_NUMERIC => parse_numeric(text),
        OID_JSON => serde_json::from_str(text)
            .map(Value::Json)
            .map_err(|_| ExtendedQueryError::protocol("invalid JSON bind parameter")),
        OID_DATE => canonical_text(text, canonical_date, "invalid date bind parameter"),
        OID_TIME => canonical_text(text, canonical_time, "invalid time bind parameter"),
        OID_TIMESTAMP => canonical_text(
            text,
            canonical_timestamp,
            "invalid timestamp bind parameter",
        ),
        OID_UUID => uuid::Uuid::parse_str(text.trim())
            .map(|uuid| Value::String(uuid.to_string()))
            .map_err(|_| ExtendedQueryError::protocol("invalid uuid bind parameter")),
        OID_BYTEA => decode_bytea(text)
            .map(|bytes| Value::String(hex_bytea(&bytes)))
            .map_err(|_| ExtendedQueryError::protocol("invalid bytea bind parameter")),
        _ => match crate::sql::binder::parameter_data_type_for_oid(oid) {
            Some(DataType::Array(element_type)) => {
                crate::types::array::parse_text_array(text, &element_type)
                    .map(Value::Json)
                    .map_err(|error| match error {
                        crate::types::array::TextArrayError::Invalid(_) => {
                            ExtendedQueryError::protocol("invalid text array parameter")
                        }
                        crate::types::array::TextArrayError::UnsupportedJsonDocumentNull => {
                            ExtendedQueryError::unsupported(error.to_string())
                        }
                    })
            }
            _ => Ok(Value::String(text.to_string())),
        },
    }
}

/// Accepts the spellings PostgreSQL's `boolin` accepts: any unique prefix of
/// `true`/`false`/`yes`/`no`, `on`/`off`, and `1`/`0`, case-insensitively and
/// with surrounding whitespace ignored.
pub(super) fn parse_bool(text: &str) -> Result<bool, ExtendedQueryError> {
    crate::types::boolean::parse_text(text)
        .ok_or_else(|| ExtendedQueryError::protocol("invalid boolean bind parameter"))
}

fn parse_integer(text: &str, min: i64, max: i64) -> Result<Value, ExtendedQueryError> {
    text.trim()
        .parse::<i64>()
        .ok()
        .filter(|value| (min..=max).contains(value))
        .map(Value::Int64)
        .ok_or_else(|| ExtendedQueryError::protocol("invalid integer bind parameter"))
}

fn parse_float(text: &str) -> Result<f64, ExtendedQueryError> {
    text.trim()
        .parse::<f64>()
        .map_err(|_| ExtendedQueryError::protocol("invalid float bind parameter"))
}

/// NUMERIC has no exact storage type, so integral text keeps full `i64`
/// precision and any other valid number decodes as a float.
fn parse_numeric(text: &str) -> Result<Value, ExtendedQueryError> {
    let text = text.trim();
    if let Ok(value) = text.parse::<i64>() {
        return Ok(Value::Int64(value));
    }
    text.parse::<f64>()
        .map(Value::Float64)
        .map_err(|_| ExtendedQueryError::protocol("invalid numeric bind parameter"))
}

fn canonical_text(
    text: &str,
    canonicalize: fn(&str) -> Result<String, String>,
    error: &str,
) -> Result<Value, ExtendedQueryError> {
    canonicalize(text.trim())
        .map(Value::String)
        .map_err(|_| ExtendedQueryError::protocol(error))
}
