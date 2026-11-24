use cassie_core::types::{DataType, DomainType, EnumType, EnumValue, SqlArray, Value, composite::{CompositeType, RECORD_STORAGE_KEY}};
use super::RowBlobValueError;
use uuid::Uuid;

use super::{
    TYPE_ARRAY, TYPE_BOOL, TYPE_BYTEA, TYPE_COMPOSITE, TYPE_DATE, TYPE_ENUM, TYPE_EXTENDED,
    TYPE_F64, TYPE_I64, TYPE_JSON, TYPE_NULL, TYPE_STRING, TYPE_TIME, TYPE_TIMESTAMP, TYPE_UUID,
    TYPE_VECTOR_F32,
};

pub fn encode_compact_value(
    data_type: &DataType,
    value: &serde_json::Value,
) -> Result<Vec<u8>, RowBlobValueError> {
    let (tag, payload) = encode_value(data_type, value)?;
    let mut bytes = Vec::with_capacity(payload.len().saturating_add(1));
    bytes.push(tag);
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

pub(super) fn encode_value(
    data_type: &DataType,
    value: &serde_json::Value,
) -> Result<(u8, Vec<u8>), RowBlobValueError> {
    if value.is_null() && !matches!(data_type, DataType::Json | DataType::Jsonb) {
        return Ok((TYPE_NULL, Vec::new()));
    }

    match data_type {
        DataType::Null => Ok((TYPE_NULL, Vec::new())),
        DataType::Extended(kind) => {
            let typed = kind.from_json(value).map_err(|error| RowBlobValueError::TypeInput { sqlstate: error.sqlstate, message: error.message })?;
            let mut bytes = kind.type_oid().to_be_bytes().to_vec();
            bytes.extend_from_slice(&typed.send_binary());
            Ok((TYPE_EXTENDED, bytes))
        }
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
        DataType::ObjectId(_) => encode_signed_integer(
            value,
            "oid field expects an unsigned 32-bit integer",
            0,
            cassie_core::types::object_id::MAX_OID,
        ),
        DataType::Float => encode_float(value),
        DataType::Boolean => encode_boolean(value),
        DataType::Name => encode_string_value(value, "name field expects string"),
        DataType::Text => encode_string_value(value, "text field expects string"),
        DataType::Char { length } => encode_bounded_string(
            value,
            *length,
            Some(1),
            cassie_core::types::char_text::canonical_char_text,
            "char field expects string",
            "char field expects up to {length} characters",
        ),
        DataType::Varchar { length } => encode_bounded_string(
            value,
            *length,
            None,
            |text| text,
            "varchar field expects string",
            "varchar field expects up to {length} characters",
        ),
        DataType::Uuid => encode_uuid(value),
        DataType::Date => encode_temporal_string(
            value,
            TYPE_DATE,
            "date field expects string",
            cassie_core::types::temporal::canonical_date,
        ),
        DataType::Time => encode_temporal_string(
            value,
            TYPE_TIME,
            "time field expects string",
            cassie_core::types::temporal::canonical_time,
        ),
        DataType::Timestamp => encode_temporal_string(
            value,
            TYPE_TIMESTAMP,
            "timestamp field expects string",
            cassie_core::types::temporal::canonical_timestamp,
        ),
        DataType::Bytea => encode_bytea(value),
        DataType::Array(inner) => encode_array_value(inner, value),
        DataType::Composite(descriptor) => encode_composite(descriptor, value),
        DataType::Domain(descriptor) => encode_domain(descriptor, value),
        DataType::Enum(descriptor) => encode_enum(descriptor, value),
        DataType::Json | DataType::Jsonb => {
            let encoded = serde_json::to_vec(value)
                .map_err(|error| RowBlobValueError::InvalidEncoding(format!("invalid json field: {error}")))?;
            Ok((TYPE_JSON, encoded))
        }
        DataType::Vector(dimensions) => encode_vector_value(*dimensions, value),
        DataType::JsonPath => Err(RowBlobValueError::Unsupported(
            "JSONPATH values are expression-only and cannot be persisted".to_string(),
        )),
    }
}

fn encode_float(value: &serde_json::Value) -> Result<(u8, Vec<u8>), RowBlobValueError> {
    let value = value
        .as_f64()
        .ok_or_else(|| RowBlobValueError::InvalidValue("float field expects f64".into()))?;
    Ok((TYPE_F64, value.to_be_bytes().to_vec()))
}

fn encode_boolean(value: &serde_json::Value) -> Result<(u8, Vec<u8>), RowBlobValueError> {
    let value = value
        .as_bool()
        .ok_or_else(|| RowBlobValueError::InvalidValue("boolean field expects bool".into()))?;
    Ok((TYPE_BOOL, vec![u8::from(value)]))
}

fn encode_uuid(value: &serde_json::Value) -> Result<(u8, Vec<u8>), RowBlobValueError> {
    let value = value
        .as_str()
        .ok_or_else(|| RowBlobValueError::InvalidValue("uuid field expects string".into()))?;
    let uuid = Uuid::parse_str(value)
        .map_err(|_| RowBlobValueError::InvalidValue("uuid field expects UUID".into()))?;
    Ok((TYPE_UUID, uuid.as_bytes().to_vec()))
}

fn encode_bytea(value: &serde_json::Value) -> Result<(u8, Vec<u8>), RowBlobValueError> {
    let value = value
        .as_str()
        .ok_or_else(|| RowBlobValueError::InvalidValue("bytea field expects string".into()))?;
    Ok((TYPE_BYTEA, decode_bytea(value)?))
}

fn encode_signed_integer(
    value: &serde_json::Value,
    error: &str,
    min: impl Into<i64>,
    max: impl Into<i64>,
) -> Result<(u8, Vec<u8>), RowBlobValueError> {
    let value = json_i64(value, error)?;
    let min = min.into();
    let max = max.into();
    if !(min..=max).contains(&value) {
        return Err(RowBlobValueError::InvalidValue(error.to_string()));
    }
    Ok((TYPE_I64, value.to_be_bytes().to_vec()))
}

fn json_i64(value: &serde_json::Value, error: &str) -> Result<i64, RowBlobValueError> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .ok_or_else(|| RowBlobValueError::InvalidValue(error.to_string()))
}

fn encode_string_value(
    value: &serde_json::Value,
    error: &str,
) -> Result<(u8, Vec<u8>), RowBlobValueError> {
    encode_typed_string(value, TYPE_STRING, error)
}

fn encode_typed_string(
    value: &serde_json::Value,
    type_tag: u8,
    error: &str,
) -> Result<(u8, Vec<u8>), RowBlobValueError> {
    let value = value
        .as_str()
        .ok_or_else(|| RowBlobValueError::InvalidValue(error.to_string()))?;
    Ok((type_tag, value.as_bytes().to_vec()))
}

/// Validates a DATE/TIME/TIMESTAMP string with `canonicalize` and stores the
/// canonical form, so every stored temporal value uses one shared parser and
/// one representation regardless of how it was originally formatted.
fn encode_temporal_string(
    value: &serde_json::Value,
    type_tag: u8,
    error: &str,
    canonicalize: impl Fn(&str) -> Result<String, String>,
) -> Result<(u8, Vec<u8>), RowBlobValueError> {
    let value = value
        .as_str()
        .ok_or_else(|| RowBlobValueError::InvalidValue(error.to_string()))?;
    let canonical = canonicalize(value).map_err(RowBlobValueError::InvalidValue)?;
    Ok((type_tag, canonical.into_bytes()))
}

fn encode_bounded_string(
    value: &serde_json::Value,
    length: Option<u32>,
    default_length: Option<u32>,
    canonicalize: fn(&str) -> &str,
    type_error: &str,
    length_error: &str,
) -> Result<(u8, Vec<u8>), RowBlobValueError> {
    let value = canonicalize(
        value
            .as_str()
            .ok_or_else(|| RowBlobValueError::InvalidValue(type_error.to_string()))?,
    );
    if let Some(length) = length.or(default_length) {
        let max_chars = usize::try_from(length).expect("string length limits fit in usize");
        if value.chars().count() > max_chars {
            return Err(RowBlobValueError::InvalidValue(
                length_error.replace("{length}", &length.to_string()),
            ));
        }
    }
    Ok((TYPE_STRING, value.as_bytes().to_vec()))
}

fn encode_array_value(
    inner: &DataType,
    value: &serde_json::Value,
) -> Result<(u8, Vec<u8>), RowBlobValueError> {
    use cassie_core::types::SqlArray;

    let mut element_type = inner;
    while let DataType::Array(nested) = element_type {
        element_type = nested;
    }

    let array = if value
        .as_object()
        .and_then(|object| object.get("$cassie_array_v1"))
        .is_some()
    {
        SqlArray::from_storage_json(value).map_err(RowBlobValueError::InvalidValue)?
    } else {
        SqlArray::from_nested_json_typed(value, element_type).map_err(RowBlobValueError::InvalidValue)?
    };

    let mut out = Vec::new();
    write_varint(
        u64::try_from(array.dimensions().len()).unwrap_or(u64::MAX),
        &mut out,
    );
    for dimension in array.dimensions() {
        out.extend_from_slice(&dimension.length.to_be_bytes());
        out.extend_from_slice(&dimension.lower_bound.to_be_bytes());
    }
    write_varint(u64::try_from(array.len()).unwrap_or(u64::MAX), &mut out);
    for (json_value, value) in array.elements_as_json().iter().zip(array.elements()) {
        let encoded_value = match value {
            cassie_core::types::Value::Null => serde_json::Value::Null,
            cassie_core::types::Value::Json(document) => document.clone(),
            _ => json_value.clone(),
        };
        let (value_type, value_data) = if matches!(value, cassie_core::types::Value::Null) {
            (TYPE_NULL, Vec::new())
        } else {
            encode_value(element_type, &encoded_value)?
        };
        out.push(value_type);
        match value_type {
            TYPE_BOOL | TYPE_I64 | TYPE_F64 | TYPE_UUID => out.extend_from_slice(&value_data),
            TYPE_NULL | TYPE_STRING | TYPE_JSON | TYPE_VECTOR_F32 | TYPE_DATE | TYPE_TIME
            | TYPE_TIMESTAMP | TYPE_ARRAY | TYPE_BYTEA | TYPE_EXTENDED | TYPE_ENUM
            | TYPE_COMPOSITE => {
                write_varint(
                    u64::try_from(value_data.len()).unwrap_or(u64::MAX),
                    &mut out,
                );
                out.extend_from_slice(&value_data);
            }
            _ => {
                return Err(RowBlobValueError::InvalidEncoding(format!(
                    "unsupported array element type tag {value_type}"
                )));
            }
        }
    }

    Ok((TYPE_ARRAY, out))
}

fn encode_vector_value(
    expected_dimensions: Option<usize>,
    value: &serde_json::Value,
) -> Result<(u8, Vec<u8>), RowBlobValueError> {
    let values = value
        .as_array()
        .ok_or_else(|| RowBlobValueError::InvalidValue("vector field expects array".into()))?;
    let dimensions = values.len();
    if !(1..=16_000).contains(&dimensions)
        || expected_dimensions.is_some_and(|expected| dimensions != expected)
    {
        return Err(RowBlobValueError::InvalidValue(format!(
            "vector field expects {} dimensions but received {dimensions}",
            expected_dimensions.map_or_else(
                || "between 1 and 16000".into(),
                |expected| { expected.to_string() }
            )
        )));
    }

    let encoded_dimensions = u32::try_from(dimensions).map_err(|_| {
        RowBlobValueError::InvalidValue("vector field expects dimensions that fit u32".into())
    })?;
    let mut out = Vec::with_capacity(4 + values.len() * 4);
    out.extend_from_slice(&encoded_dimensions.to_be_bytes());
    for value in values {
        let value = value.as_f64().ok_or_else(|| {
            RowBlobValueError::InvalidValue("vector field expects numeric values".into())
        })?;
        if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
            return Err(RowBlobValueError::InvalidValue(
                "vector field expects finite f32-range values".into(),
            ));
        }
        let encoded = value.to_string().parse::<f32>().map_err(|_| {
            RowBlobValueError::InvalidValue("vector field expects f32-range values".into())
        })?;
        out.extend_from_slice(&encoded.to_be_bytes());
    }
    Ok((TYPE_VECTOR_F32, out))
}

fn encode_composite(
    descriptor: &CompositeType,
    value: &serde_json::Value,
) -> Result<(u8, Vec<u8>), RowBlobValueError> {
    let invalid = || RowBlobValueError::InvalidValue(format!(
        "composite field expects a record of type {}",
        descriptor.name
    ));
    let body = value.as_object()
        .and_then(|object| object.get(RECORD_STORAGE_KEY))
        .and_then(serde_json::Value::as_object)
        .ok_or_else(invalid)?;
    if body.get("oid").and_then(serde_json::Value::as_u64) != Some(u64::from(descriptor.oid)) {
        return Err(invalid());
    }
    let fields = body.get("fields").and_then(serde_json::Value::as_array).ok_or_else(invalid)?;
    if fields.len() > descriptor.physical_len() { return Err(invalid()); }
    let mut out = vec![1];
    out.extend_from_slice(&descriptor.oid.to_be_bytes());
    out.extend_from_slice(&descriptor.revision.to_be_bytes());
    let count = u32::try_from(descriptor.physical_len()).map_err(|_| RowBlobValueError::InvalidEncoding("composite field count overflow".to_owned()))?;
    out.extend_from_slice(&count.to_be_bytes());
    let mut live = descriptor.fields.iter();
    for slot in 0..descriptor.physical_len() {
        let stored = fields.get(slot).unwrap_or(&serde_json::Value::Null);
        let field = if descriptor.dropped_positions.contains(&slot) { None } else { live.next() };
        let (tag, data) = match field {
            Some(field) if !stored.is_null() => encode_value(&field.data_type, stored)?,
            _ => (TYPE_NULL, Vec::new()),
        };
        out.push(tag);
        match tag {
            TYPE_BOOL | TYPE_I64 | TYPE_F64 | TYPE_UUID | TYPE_ARRAY => out.extend_from_slice(&data),
            _ => { write_varint(u64::try_from(data.len()).unwrap_or(u64::MAX), &mut out); out.extend_from_slice(&data); }
        }
    }
    Ok((TYPE_COMPOSITE, out))
}

fn encode_domain(domain: &DomainType, value: &serde_json::Value) -> Result<(u8, Vec<u8>), RowBlobValueError> {
    let (tag, data) = encode_value(&domain.base, value)?;
    if tag == TYPE_NULL { return Err(RowBlobValueError::InvalidEncoding(format!("domain {} cannot wrap a NULL base value", domain.name))); }
    let mut out = vec![1]; out.extend_from_slice(&domain.oid.to_be_bytes()); out.extend_from_slice(&domain.revision.to_be_bytes()); out.push(tag);
    match tag {
        TYPE_BOOL | TYPE_I64 | TYPE_F64 | TYPE_UUID | TYPE_ARRAY => out.extend_from_slice(&data),
        _ => { write_varint(u64::try_from(data.len()).unwrap_or(u64::MAX), &mut out); out.extend_from_slice(&data); }
    }
    Ok((TYPE_DOMAIN, out))
}

fn encode_enum(descriptor: &EnumType, value: &serde_json::Value) -> Result<(u8, Vec<u8>), RowBlobValueError> {
    let member_id = EnumValue::storage_member_id(value)
        .filter(|member_id| descriptor.member(*member_id).is_some())
        .ok_or_else(|| RowBlobValueError::InvalidValue(format!("enum field expects a member of type {}", descriptor.name)))?;
    let mut out = vec![1]; out.extend_from_slice(&descriptor.oid.to_be_bytes()); out.extend_from_slice(&member_id.to_be_bytes());
    Ok((TYPE_ENUM, out))
}

/// Canonicalizes a hexadecimal bytea field using the row encoder's validation.
///
/// # Errors
/// Returns an error when the field is not valid hexadecimal bytea.
pub fn canonical_bytea_text(value: &str) -> Result<String, RowBlobValueError> {
    let bytes = decode_bytea(value)?;
    Ok(super::decode::encode_bytea(&bytes))
}

fn decode_bytea(value: &str) -> Result<Vec<u8>, RowBlobValueError> {
    if !value.starts_with("\\x") { return Err(RowBlobValueError::InvalidValue("bytea field expects '\\x' hexadecimal format".to_owned())); }
    if value.len() == 2 { return Ok(Vec::new()); }
    if (value.len() - 2).rem_euclid(2) != 0 { return Err(RowBlobValueError::InvalidValue("bytea field expects an even number of hex digits".to_owned())); }
    let bytes = value.as_bytes(); let mut out = Vec::with_capacity((value.len() - 2) / 2); let mut index = 2;
    while index < value.len() {
        let high = decode_hex_digit(bytes[index]).ok_or_else(|| RowBlobValueError::InvalidValue("bytea field expects hexadecimal input".to_owned()))?;
        let low = decode_hex_digit(bytes[index + 1]).ok_or_else(|| RowBlobValueError::InvalidValue("bytea field expects hexadecimal input".to_owned()))?;
        out.push((high << 4) | low); index += 2;
    }
    Ok(out)
}

fn decode_hex_digit(byte: u8) -> Option<u8> { match byte { b'0'..=b'9' => Some(byte - b'0'), b'a'..=b'f' => Some(byte - b'a' + 10), b'A'..=b'F' => Some(byte - b'A' + 10), _ => None } }

fn write_varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        let byte = u8::try_from(value & 0x7f).expect("masked to 7 bits");
        out.push(byte | 0x80);
        value >>= 7;
    }
    out.push(u8::try_from(value).expect("final varint byte fits in u8"));
}
