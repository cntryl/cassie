use crate::app::CassieError;
use crate::types::DataType;
use uuid::Uuid;

use super::{
    decode_bytea, write_varint, TYPE_ARRAY, TYPE_BOOL, TYPE_BYTEA, TYPE_DATE, TYPE_F64, TYPE_I64,
    TYPE_JSON, TYPE_NULL, TYPE_STRING, TYPE_TIME, TYPE_TIMESTAMP, TYPE_UUID, TYPE_VECTOR_F32,
};

pub(super) fn encode_value(
    data_type: &DataType,
    value: &serde_json::Value,
) -> Result<(u8, Vec<u8>), CassieError> {
    if value.is_null() {
        return Ok((TYPE_NULL, Vec::new()));
    }

    match data_type {
        DataType::Null => Ok((TYPE_NULL, Vec::new())),
        DataType::SmallInt => {
            encode_signed_integer(value, "smallint field expects i16", i16::MIN, i16::MAX)
        }
        DataType::Int => {
            encode_signed_integer(value, "integer field expects i32", i32::MIN, i32::MAX)
        }
        DataType::BigInt => {
            let value = json_i64(value, "bigint field expects i64")?;
            Ok((TYPE_I64, value.to_be_bytes().to_vec()))
        }
        DataType::Float => {
            let value = value
                .as_f64()
                .ok_or_else(|| CassieError::InvalidVector("float field expects f64".into()))?;
            Ok((TYPE_F64, value.to_be_bytes().to_vec()))
        }
        DataType::Boolean => {
            let value = value
                .as_bool()
                .ok_or_else(|| CassieError::InvalidVector("boolean field expects bool".into()))?;
            Ok((TYPE_BOOL, vec![u8::from(value)]))
        }
        DataType::Text => encode_string_value(value, "text field expects string"),
        DataType::Char { length } => encode_bounded_string(
            value,
            *length,
            1,
            "char field expects string",
            "char field expects up to {length} characters",
        ),
        DataType::Varchar { length } => encode_bounded_string(
            value,
            *length,
            0,
            "varchar field expects string",
            "varchar field expects up to {length} characters",
        ),
        DataType::Uuid => {
            let value = value
                .as_str()
                .ok_or_else(|| CassieError::InvalidVector("uuid field expects string".into()))?;
            let uuid = Uuid::parse_str(value)
                .map_err(|_| CassieError::InvalidVector("uuid field expects UUID".into()))?;
            Ok((TYPE_UUID, uuid.as_bytes().to_vec()))
        }
        DataType::Date => encode_typed_string(value, TYPE_DATE, "date field expects string"),
        DataType::Time => encode_typed_string(value, TYPE_TIME, "time field expects string"),
        DataType::Timestamp => {
            encode_typed_string(value, TYPE_TIMESTAMP, "timestamp field expects string")
        }
        DataType::Bytea => {
            let value = value
                .as_str()
                .ok_or_else(|| CassieError::InvalidVector("bytea field expects string".into()))?;
            Ok((TYPE_BYTEA, decode_bytea(value)?))
        }
        DataType::Array(inner) => encode_array_value(inner, value),
        DataType::Json => {
            let encoded = serde_json::to_vec(value)
                .map_err(|error| CassieError::Parse(format!("invalid json field: {error}")))?;
            Ok((TYPE_JSON, encoded))
        }
        DataType::Vector(dimensions) => encode_vector_value(*dimensions, value),
    }
}

fn encode_signed_integer(
    value: &serde_json::Value,
    error: &str,
    min: impl Into<i64>,
    max: impl Into<i64>,
) -> Result<(u8, Vec<u8>), CassieError> {
    let value = json_i64(value, error)?;
    let min = min.into();
    let max = max.into();
    if !(min..=max).contains(&value) {
        return Err(CassieError::InvalidVector(error.to_string()));
    }
    Ok((TYPE_I64, value.to_be_bytes().to_vec()))
}

fn json_i64(value: &serde_json::Value, error: &str) -> Result<i64, CassieError> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .ok_or_else(|| CassieError::InvalidVector(error.to_string()))
}

fn encode_string_value(
    value: &serde_json::Value,
    error: &str,
) -> Result<(u8, Vec<u8>), CassieError> {
    encode_typed_string(value, TYPE_STRING, error)
}

fn encode_typed_string(
    value: &serde_json::Value,
    type_tag: u8,
    error: &str,
) -> Result<(u8, Vec<u8>), CassieError> {
    let value = value
        .as_str()
        .ok_or_else(|| CassieError::InvalidVector(error.to_string()))?;
    Ok((type_tag, value.as_bytes().to_vec()))
}

fn encode_bounded_string(
    value: &serde_json::Value,
    length: Option<u32>,
    default_length: u32,
    type_error: &str,
    length_error: &str,
) -> Result<(u8, Vec<u8>), CassieError> {
    let value = value
        .as_str()
        .ok_or_else(|| CassieError::InvalidVector(type_error.to_string()))?;
    let length = length.unwrap_or(default_length);
    let max_chars = usize::try_from(length).expect("string length limits fit in usize");
    if value.chars().count() > max_chars {
        return Err(CassieError::InvalidVector(
            length_error.replace("{length}", &length.to_string()),
        ));
    }
    Ok((TYPE_STRING, value.as_bytes().to_vec()))
}

fn encode_array_value(
    inner: &DataType,
    value: &serde_json::Value,
) -> Result<(u8, Vec<u8>), CassieError> {
    let values = value
        .as_array()
        .ok_or_else(|| CassieError::InvalidVector("array field expects array".into()))?;

    let mut out = Vec::new();
    write_varint(u64::try_from(values.len()).unwrap_or(u64::MAX), &mut out);
    for value in values {
        let (value_type, value_data) = encode_value(inner, value)?;
        out.push(value_type);
        match value_type {
            TYPE_BOOL | TYPE_I64 | TYPE_F64 | TYPE_UUID => out.extend_from_slice(&value_data),
            TYPE_NULL | TYPE_STRING | TYPE_JSON | TYPE_VECTOR_F32 | TYPE_DATE | TYPE_TIME
            | TYPE_TIMESTAMP | TYPE_ARRAY | TYPE_BYTEA => {
                write_varint(
                    u64::try_from(value_data.len()).unwrap_or(u64::MAX),
                    &mut out,
                );
                out.extend_from_slice(&value_data);
            }
            _ => {
                return Err(CassieError::Parse(format!(
                    "unsupported array element type tag {value_type}"
                )));
            }
        }
    }

    Ok((TYPE_ARRAY, out))
}

fn encode_vector_value(
    dimensions: usize,
    value: &serde_json::Value,
) -> Result<(u8, Vec<u8>), CassieError> {
    let values = value
        .as_array()
        .ok_or_else(|| CassieError::InvalidVector("vector field expects array".into()))?;
    if values.len() != dimensions {
        return Err(CassieError::InvalidVector(format!(
            "vector field expects {dimensions} dimensions"
        )));
    }

    let encoded_dimensions = u32::try_from(dimensions).map_err(|_| {
        CassieError::InvalidVector("vector field expects dimensions that fit u32".into())
    })?;
    let mut out = Vec::with_capacity(4 + values.len() * 4);
    out.extend_from_slice(&encoded_dimensions.to_be_bytes());
    for value in values {
        let value = value.as_f64().ok_or_else(|| {
            CassieError::InvalidVector("vector field expects numeric values".into())
        })?;
        let encoded = value.to_string().parse::<f32>().map_err(|_| {
            CassieError::InvalidVector("vector field expects f32-range values".into())
        })?;
        out.extend_from_slice(&encoded.to_be_bytes());
    }
    Ok((TYPE_VECTOR_F32, out))
}
