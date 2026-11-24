#[cfg(test)]
use std::collections::HashSet;

use crate::app::CassieError;
#[cfg(test)]
use crate::types::{ArrayDimension, DataType, FieldSchema, Schema, SqlArray, Value};
#[cfg(test)]
use uuid::Uuid;

#[cfg(any(test, feature = "transport-test-hooks"))]
use cassie_storage::row_blob::{field_bit, Cursor, RowFieldMeta, FORMAT_VERSION, MAGIC, TYPE_JSON};
pub(crate) use cassie_storage::row_blob::{RowBlobEncodeError, RowSchema};
#[cfg(test)]
use cassie_storage::row_blob::{
    RowBlobField, RowBlobRecord, TYPE_ARRAY, TYPE_BOOL, TYPE_BYTEA, TYPE_COMPOSITE, TYPE_DATE,
    TYPE_DOMAIN, TYPE_ENUM, TYPE_EXTENDED, TYPE_F64, TYPE_I64, TYPE_NULL, TYPE_STRING, TYPE_TIME,
    TYPE_TIMESTAMP, TYPE_UUID, TYPE_VECTOR_F32,
};

mod compact;
#[cfg(test)]
mod composite;
#[cfg(test)]
mod domain;
#[cfg(test)]
mod encoding;
#[cfg(test)]
mod enum_value;
mod vector_field;

#[cfg(test)]
use self::compact::decode_directory_value;
#[cfg(any(test, feature = "transport-test-hooks"))]
use self::compact::expected_type_tag;
pub(crate) use compact::encode_compact_value;
pub(crate) use vector_field::VectorColumn;

pub(crate) fn encode_row(
    schema: &RowSchema,
    payload: &serde_json::Value,
) -> Result<Vec<u8>, CassieError> {
    cassie_storage::row_blob::encode_row(schema, payload).map_err(|error| match error {
        RowBlobEncodeError::Layout(error) => CassieError::from(error),
        RowBlobEncodeError::FieldValue(error) => map_row_value_error(error),
    })
}

fn map_row_value_error(error: cassie_storage::row_blob::RowBlobValueError) -> CassieError {
    match error {
        cassie_storage::row_blob::RowBlobValueError::InvalidValue(message) => {
            CassieError::InvalidVector(message)
        }
        cassie_storage::row_blob::RowBlobValueError::InvalidEncoding(message) => {
            CassieError::Parse(message)
        }
        cassie_storage::row_blob::RowBlobValueError::Unsupported(message) => {
            CassieError::Unsupported(message)
        }
        cassie_storage::row_blob::RowBlobValueError::TypeInput { sqlstate, message } => {
            CassieError::from(crate::app::DiagnosticError::new(sqlstate, message))
        }
    }
}

pub(crate) fn decode_row(schema: &RowSchema, row: &[u8]) -> Result<serde_json::Value, CassieError> {
    cassie_storage::row_blob::decode_row(schema, row, decode_engine_json)
        .map_err(map_row_decode_error)
}

#[cfg(test)]
pub(crate) fn decode_projected_row(
    schema: &RowSchema,
    row: &[u8],
    projection: &HashSet<String>,
) -> Result<serde_json::Value, CassieError> {
    cassie_storage::row_blob::decode_projected_row(schema, row, projection, decode_engine_json)
        .map_err(map_row_decode_error)
}

#[cfg(test)]
pub(crate) fn decode_projected_row_matching(
    schema: &RowSchema,
    row: &[u8],
    projection: &HashSet<String>,
    filter_field: &str,
    filter_value: &serde_json::Value,
) -> Result<Option<serde_json::Value>, CassieError> {
    cassie_storage::row_blob::decode_projected_row_matching(
        schema,
        row,
        projection,
        filter_field,
        filter_value,
        false,
        decode_engine_json,
    )
    .map_err(map_row_decode_error)
}

pub(crate) fn decode_engine_json(bytes: &[u8]) -> Result<serde_json::Value, CassieError> {
    let controls = crate::midge::adapter::PublicationControlScope::current_query_controls();
    match crate::runtime::accounted::json_arena::parse_value_with_optional_controls(
        bytes,
        controls.as_ref(),
    ) {
        Ok(parsed) => Ok(parsed.get().clone()),
        Err(crate::runtime::accounted::json_arena::ValueParseError::Controlled(error)) => {
            Err(error)
        }
        Err(crate::runtime::accounted::json_arena::ValueParseError::Invalid) => {
            let detail = crate::runtime::accounted::json_arena::syntax_error(bytes);
            Err(CassieError::Parse(format!(
                "invalid JSON in row blob: {detail}"
            )))
        }
    }
}

fn map_row_decode_error(
    error: cassie_storage::row_blob::RowBlobDecodeError<CassieError>,
) -> CassieError {
    match error {
        cassie_storage::row_blob::RowBlobDecodeError::Codec(error) => error.into(),
        cassie_storage::row_blob::RowBlobDecodeError::Json(error) => error,
        cassie_storage::row_blob::RowBlobDecodeError::DataCorruption { sqlstate, message } => {
            CassieError::from(crate::app::DiagnosticError::new(sqlstate, message))
        }
    }
}

#[cfg(test)]
struct RowDirectory<'a>(RowBlobRecord<'a>);

#[cfg(test)]
impl<'a> RowDirectory<'a> {
    fn parse(row: &'a [u8]) -> Result<Self, CassieError> {
        RowBlobRecord::parse(row)
            .map(Self)
            .map_err(CassieError::from)
    }

    fn decode(&self, field: &RowFieldMeta) -> Result<Option<serde_json::Value>, CassieError> {
        let Some(stored) = self.0.field(field.field_id)? else {
            return Ok(None);
        };
        let (type_tag, bytes) = match stored {
            RowBlobField::SqlNull => {
                if matches!(field.data_type, DataType::Json | DataType::Jsonb) {
                    // Older rows encoded JSON document null with the generic SQL
                    // NULL tag. Under the current contract those rows are SQL NULL.
                    return Ok(None);
                }
                return Ok(Some(serde_json::Value::Null));
            }
            RowBlobField::Encoded { type_tag, bytes } => (type_tag, bytes),
        };
        // Several tags differ by a single bit (TYPE_F64/TYPE_STRING,
        // TYPE_UUID/TYPE_JSON), so a corrupted tag can decode as a valid
        // value of the wrong type. The schema already says what this field
        // must be; disagreement means the blob is not what it claims.
        let expected = expected_type_tag(&field.data_type);
        if type_tag != expected {
            return Err(CassieError::Parse(format!(
                "row blob field '{}' stores type tag {type_tag} but its schema declares type tag {expected}",
                field.name
            )));
        }
        let decoded = decode_directory_value(type_tag, bytes)?;
        if let DataType::Composite(descriptor) = &field.data_type {
            composite::check_stored(descriptor, &decoded)?;
        }
        enum_value::check_column(&field.data_type, &decoded)?;
        Ok(Some(decoded))
    }
}

#[cfg(test)]
fn decode_value(
    type_tag: u8,
    cursor: &mut Cursor<'_>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<serde_json::Value, CassieError> {
    match type_tag {
        TYPE_NULL => Ok(serde_json::Value::Null),
        TYPE_BOOL => Ok(serde_json::Value::Bool(cursor.read_u8()? != 0)),
        TYPE_UUID => {
            let bytes = cursor.read_exact(16)?;
            let uuid = Uuid::from_slice(bytes).map_err(|error| {
                CassieError::Parse(format!("invalid UUID in row blob: {error}"))
            })?;
            Ok(serde_json::Value::String(uuid.to_string()))
        }
        TYPE_I64 => {
            let value = cursor.read_i64()?;
            Ok(serde_json::Value::Number(value.into()))
        }
        TYPE_F64 => {
            let value = cursor.read_f64()?;
            serde_json::Number::from_f64(value)
                .map(serde_json::Value::Number)
                .ok_or_else(|| CassieError::Parse("invalid f64 in row blob".to_string()))
        }
        TYPE_STRING => {
            let bytes = cursor.read_len_prefixed()?;
            String::from_utf8(bytes)
                .map(serde_json::Value::String)
                .map_err(|error| CassieError::Parse(format!("invalid UTF-8 in row blob: {error}")))
        }
        TYPE_JSON => decode_json_value(cursor, controls),
        TYPE_DATE => {
            let bytes = cursor.read_len_prefixed()?;
            String::from_utf8(bytes)
                .map(serde_json::Value::String)
                .map_err(|error| CassieError::Parse(format!("invalid date in row blob: {error}")))
        }
        TYPE_TIME => {
            let bytes = cursor.read_len_prefixed()?;
            String::from_utf8(bytes)
                .map(serde_json::Value::String)
                .map_err(|error| CassieError::Parse(format!("invalid time in row blob: {error}")))
        }
        TYPE_TIMESTAMP => {
            let bytes = cursor.read_len_prefixed()?;
            String::from_utf8(bytes)
                .map(serde_json::Value::String)
                .map_err(|error| {
                    CassieError::Parse(format!("invalid timestamp in row blob: {error}"))
                })
        }
        TYPE_BYTEA => {
            let bytes = cursor.read_len_prefixed()?;
            Ok(serde_json::Value::String(encode_bytea(&bytes)))
        }
        TYPE_ARRAY => decode_array(cursor, controls),
        TYPE_COMPOSITE => composite::decode_composite(cursor, controls),
        TYPE_DOMAIN => domain::decode_domain(cursor, controls),
        TYPE_ENUM => enum_value::decode_enum(cursor),
        TYPE_EXTENDED => {
            let bytes = cursor.read_len_prefixed()?;
            let (oid, payload) = bytes
                .split_first_chunk::<4>()
                .ok_or_else(|| CassieError::Parse("truncated typed value in row blob".into()))?;
            let kind = crate::types::extended::ExtendedType::for_oid(u32::from_be_bytes(*oid))
                .ok_or_else(|| CassieError::Parse("unknown typed value in row blob".into()))?;
            kind.receive_binary(payload)
                .map(|typed| typed.to_json())
                .map_err(CassieError::Parse)
        }
        TYPE_VECTOR_F32 => {
            let bytes = cursor.read_len_prefixed()?;
            if bytes.len() < 4 {
                return Err(CassieError::Parse(
                    "invalid vector field in row blob".into(),
                ));
            }
            let dimensions = u32::from_be_bytes(bytes[0..4].try_into().map_err(|_| {
                CassieError::Parse("invalid vector dimension in row blob".to_string())
            })?) as usize;
            let expected_len = 4 + dimensions * 4;
            if bytes.len() != expected_len {
                return Err(CassieError::Parse(
                    "invalid vector length in row blob".into(),
                ));
            }

            let mut values = Vec::with_capacity(dimensions);
            for chunk in bytes[4..].as_chunks::<4>().0 {
                let value = f32::from_be_bytes(*chunk);
                let number = serde_json::Number::from_f64(f64::from(value)).ok_or_else(|| {
                    CassieError::Parse("invalid vector numeric value in row blob".to_string())
                })?;
                values.push(serde_json::Value::Number(number));
            }
            Ok(serde_json::Value::Array(values))
        }
        _ => Err(CassieError::Parse(format!(
            "unsupported row blob type tag {type_tag}"
        ))),
    }
}

#[cfg(test)]
fn decode_json_value(
    cursor: &mut Cursor<'_>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<serde_json::Value, CassieError> {
    let bytes = cursor.read_len_prefixed()?;
    match crate::runtime::accounted::json_arena::parse_value_with_optional_controls(
        &bytes, controls,
    ) {
        Ok(parsed) => Ok(parsed.get().clone()),
        Err(crate::runtime::accounted::json_arena::ValueParseError::Controlled(error)) => {
            Err(error)
        }
        Err(crate::runtime::accounted::json_arena::ValueParseError::Invalid) => {
            let detail = crate::runtime::accounted::json_arena::syntax_error(&bytes);
            Err(CassieError::Parse(format!(
                "invalid JSON in row blob: {detail}"
            )))
        }
    }
}

#[cfg(test)]
fn decode_array(
    cursor: &mut Cursor<'_>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<serde_json::Value, CassieError> {
    let dimension_count = usize::try_from(cursor.read_varint()?).map_err(|_| {
        CassieError::Parse("array dimension count out of range in row blob".to_string())
    })?;
    if dimension_count > cursor.remaining().len() / 8 {
        return Err(CassieError::Parse(
            "array dimension count exceeds remaining payload in row blob".to_string(),
        ));
    }
    let mut dimensions = Vec::with_capacity(dimension_count);
    for _ in 0..dimension_count {
        let length = i32::from_be_bytes(cursor.read_exact(4)?.try_into().map_err(|_| {
            CassieError::Parse("invalid array dimension length in row blob".to_string())
        })?);
        let lower_bound = i32::from_be_bytes(cursor.read_exact(4)?.try_into().map_err(|_| {
            CassieError::Parse("invalid array lower bound in row blob".to_string())
        })?);
        dimensions.push(ArrayDimension {
            length,
            lower_bound,
        });
    }
    let count = usize::try_from(cursor.read_varint()?)
        .map_err(|_| CassieError::Parse("array length out of range in row blob".to_string()))?;
    // Even null, boolean and empty variable-width elements require two bytes.
    if count > cursor.remaining().len() / 2 {
        return Err(CassieError::Parse(
            "array count exceeds remaining payload in row blob".to_string(),
        ));
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let value_type = cursor.read_u8()?;
        let value = match value_type {
            TYPE_NULL => {
                let payload_len = cursor.read_varint()?;
                if payload_len != 0 {
                    return Err(CassieError::Parse(
                        "null array element carries a nonzero payload length".to_string(),
                    ));
                }
                Value::Null
            }
            TYPE_ARRAY => {
                return Err(CassieError::Parse(
                    "nested array element is invalid in CRB1 row blob".to_string(),
                ));
            }
            TYPE_EXTENDED => decode_array_extended(cursor)?,
            _ => value_from_decoded_tag(value_type, decode_value(value_type, cursor, controls)?)?,
        };
        values.push(value);
    }
    let array = SqlArray::new(dimensions, values).map_err(CassieError::Parse)?;
    Ok(array.to_storage_json())
}

#[cfg(test)]
fn decode_array_extended(cursor: &mut Cursor<'_>) -> Result<Value, CassieError> {
    let bytes = cursor.read_len_prefixed()?;
    let (oid, payload) = bytes
        .split_first_chunk::<4>()
        .ok_or_else(|| CassieError::Parse("truncated typed array value in row blob".into()))?;
    let kind = crate::types::extended::ExtendedType::for_oid(u32::from_be_bytes(*oid))
        .ok_or_else(|| CassieError::Parse("unknown typed array value in row blob".into()))?;
    kind.receive_binary(payload)
        .map(Value::Typed)
        .map_err(CassieError::Parse)
}

#[cfg(test)]
fn value_from_decoded_tag(type_tag: u8, value: serde_json::Value) -> Result<Value, CassieError> {
    match type_tag {
        TYPE_BOOL => value.as_bool().map(Value::Bool),
        TYPE_I64 => value.as_i64().map(Value::Int64),
        TYPE_F64 => value.as_f64().map(Value::Float64),
        TYPE_STRING | TYPE_DATE | TYPE_TIME | TYPE_TIMESTAMP | TYPE_BYTEA | TYPE_UUID => {
            value.as_str().map(|text| Value::String(text.to_string()))
        }
        TYPE_VECTOR_F32 => value
            .as_array()
            .and_then(|elements| {
                elements
                    .iter()
                    .map(|element| {
                        element
                            .as_f64()
                            .and_then(|number| number.to_string().parse::<f32>().ok())
                    })
                    .collect::<Option<Vec<_>>>()
            })
            .map(crate::types::Vector::new)
            .map(Value::Vector),
        TYPE_JSON | TYPE_ENUM | TYPE_COMPOSITE => Some(Value::Json(value)),
        _ => None,
    }
    .ok_or_else(|| CassieError::Parse(format!("invalid array element for type tag {type_tag}")))
}

#[cfg(test)]
fn write_varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        let byte = u8::try_from(value & 0x7f).expect("masked to 7 bits");
        out.push(byte | 0x80);
        value >>= 7;
    }
    out.push(u8::try_from(value).expect("final varint byte fits in u8"));
}

pub(crate) fn canonical_bytea_text(value: &str) -> Result<String, CassieError> {
    cassie_storage::row_blob::canonical_bytea_text(value).map_err(map_row_value_error)
}

#[cfg(test)]
fn decode_bytea(value: &str) -> Result<Vec<u8>, CassieError> {
    if !value.starts_with("\\x") {
        return Err(CassieError::InvalidVector(
            "bytea field expects '\\x' hexadecimal format".to_string(),
        ));
    }
    if value.len() == 2 {
        return Ok(Vec::new());
    }
    if (value.len() - 2).rem_euclid(2) != 0 {
        return Err(CassieError::InvalidVector(
            "bytea field expects an even number of hex digits".to_string(),
        ));
    }

    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity((value.len() - 2) / 2);
    let mut index = 2;
    while index < value.len() {
        let high = decode_hex_digit(bytes[index]).ok_or_else(|| {
            CassieError::InvalidVector("bytea field expects hexadecimal input".to_string())
        })?;
        let low = decode_hex_digit(bytes[index + 1]).ok_or_else(|| {
            CassieError::InvalidVector("bytea field expects hexadecimal input".to_string())
        })?;
        out.push((high << 4) | low);
        index += 2;
    }
    Ok(out)
}

#[cfg(test)]
fn decode_hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
fn encode_bytea(value: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(2 + value.len() * 2);
    output.push_str("\\x");
    for byte in value {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(any(test, feature = "transport-test-hooks"))]
#[path = "row_blob/test_fixtures.rs"]
pub(super) mod test_fixtures;

#[cfg(test)]
#[path = "row_blob/tests.rs"]
mod tests;
