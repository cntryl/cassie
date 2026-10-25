#[path = "codecs/jsonb.rs"]
mod jsonb;

use crate::executor::ColumnMeta;
use crate::types::builtin::{
    builtin_for_oid, BuiltinKind as BinaryCodecKind, BuiltinType as BinaryCodec,
};
use crate::types::schema::vector_dimensions_for_oid;
use crate::types::Value;
use std::{io, str};
use time::{Date, PrimitiveDateTime, Time};

#[path = "codecs/vector.rs"]
mod vector;

use vector::{decode_vector, encode_vector};

const OID_ARRAY_BASE: i64 = 34_000;
const OID_ARRAY_LIMIT: i64 = 44_000;
const POSTGRES_INTERNAL_CHAR_OID: i64 = 18;
const POSTGRES_EPOCH_JULIAN_DAY: i32 = 2_451_545;
const MICROSECONDS_PER_DAY: i64 = 86_400_000_000;

/// Renders `value` with PostgreSQL's text output function for the column's
/// declared `type_oid`.
pub(super) fn value_to_text(value: &Value, type_oid: i64) -> String {
    if type_oid == 3802 {
        return jsonb::text(value);
    }
    match value {
        Value::Null => "NULL".to_string(),
        // boolout emits a single `t`/`f`; a JSON column keeps JSON's spelling.
        Value::Bool(v) if type_oid == i64::from(BinaryCodecKind::Bool.metadata().oid) => {
            if *v { "t" } else { "f" }.to_string()
        }
        Value::Bool(v) => v.to_string(),
        Value::Int64(v) => v.to_string(),
        Value::Float64(v) if type_oid == 701 || !v.is_finite() => {
            crate::types::float_text::format(*v)
        }
        Value::Float64(v) => v.to_string(),
        Value::String(v)
            if type_oid == 1114 && crate::types::temporal::is_canonical_timestamp_text(v) =>
        {
            crate::types::temporal::timestamp_text(v).unwrap_or_else(|_| v.clone())
        }
        Value::String(v) if type_oid == 1083 => {
            crate::types::temporal::time_text(v).unwrap_or_else(|_| v.clone())
        }
        Value::String(v) => v.clone(),
        Value::Vector(v) => format!(
            "[{}]",
            v.values
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Json(v) => v.to_string(),
        Value::Typed(v) => v.text(),
    }
}

pub(super) fn value_to_postgres_array_text(value: &Value, type_oid: i64) -> Option<String> {
    let Value::Json(array) = value else {
        return None;
    };
    let element_oid = u32::try_from(builtin_array_element_oid(type_oid)?).ok()?;
    let element_type = crate::types::builtin::builtin_for_oid(element_oid)?
        .kind
        .data_type();
    crate::types::array::format_text_array(array, &element_type)
}

pub(super) fn value_to_binary(value: &Value, type_oid: i64) -> io::Result<Vec<u8>> {
    if type_oid == 4073 {
        return Err(unsupported_codec(type_oid));
    }
    if type_oid == POSTGRES_INTERNAL_CHAR_OID {
        return match value {
            Value::String(value) if value.len() == 1 => Ok(value.as_bytes().to_vec()),
            _ => invalid_value("char"),
        };
    }
    if type_oid == 2278 {
        return match value {
            Value::String(value) if value.is_empty() => Ok(Vec::new()),
            _ => Err(invalid_data("void")),
        };
    }
    if vector_dimensions_for_oid(type_oid).is_some() {
        return encode_vector(value, type_oid);
    }
    if (OID_ARRAY_BASE..OID_ARRAY_LIMIT).contains(&type_oid) {
        return encode_array(value, type_oid, type_oid - OID_ARRAY_BASE);
    }
    if let Some(element_oid) = builtin_array_element_oid(type_oid) {
        return encode_array(value, type_oid, element_oid);
    }
    let codec = binary_codec_for_oid(type_oid)?;
    match codec.kind {
        BinaryCodecKind::Bool => match value {
            Value::Bool(value) => Ok(vec![u8::from(*value)]),
            Value::String(value) => parse_bool_binary(value),
            _ => invalid_value(codec.name),
        },
        BinaryCodecKind::Bytea => match value {
            Value::String(value) => decode_bytea(value),
            _ => invalid_value(codec.name),
        },
        BinaryCodecKind::Int2 => encode_integer(value, codec.name, encode_i16_binary),
        BinaryCodecKind::Int4 => encode_integer(value, codec.name, encode_i32_binary),
        BinaryCodecKind::Int8 => encode_int8(value, codec.name),
        BinaryCodecKind::ObjectId(_) => encode_integer(value, codec.name, encode_oid_binary),
        BinaryCodecKind::Extended(kind) => match value {
            Value::Typed(typed) => Ok(typed.send_binary()),
            other => kind
                .cast_from(other)
                .map(|typed| typed.send_binary())
                .map_err(|_| invalid_data(codec.name)),
        },
        BinaryCodecKind::Float8 => encode_float8(value, codec.name),
        BinaryCodecKind::Text
        | BinaryCodecKind::Bpchar
        | BinaryCodecKind::Varchar
        | BinaryCodecKind::Unknown => Ok(value_to_text(value, type_oid).into_bytes()),
        // Binary json is the document's text. Reads unwrap JSON scalars to
        // plain values, so a number or boolean is written in JSON spelling.
        BinaryCodecKind::Jsonb => jsonb::encode(value),
        BinaryCodecKind::Json => match value {
            Value::Json(value) => Ok(value.to_string().into_bytes()),
            Value::String(value) => Ok(value.as_bytes().to_vec()),
            Value::Bool(value) => Ok(value.to_string().into_bytes()),
            Value::Int64(value) => Ok(value.to_string().into_bytes()),
            Value::Float64(value) => serde_json::Number::from_f64(*value)
                .map(|number| number.to_string().into_bytes())
                .ok_or_else(|| invalid_data(codec.name)),
            _ => invalid_value(codec.name),
        },
        BinaryCodecKind::JsonPath => match value {
            Value::String(value) => {
                let path = sql_json_path::JsonPath::new(value)
                    .map_err(|_| invalid_data(codec.name))?
                    .to_string();
                let mut bytes = Vec::with_capacity(path.len().saturating_add(1));
                bytes.push(1);
                bytes.extend_from_slice(path.as_bytes());
                Ok(bytes)
            }
            _ => invalid_value(codec.name),
        },
        BinaryCodecKind::Uuid => match value {
            Value::String(value) => uuid::Uuid::parse_str(value)
                .map(|value| value.into_bytes().to_vec())
                .map_err(|_| invalid_data(codec.name)),
            _ => invalid_value(codec.name),
        },
        BinaryCodecKind::Date | BinaryCodecKind::Time | BinaryCodecKind::Timestamp => {
            native_temporal(codec.kind).map_or_else(
                || invalid_value(codec.name),
                |kind| match value {
                    Value::Typed(typed) => Ok(typed.send_binary()),
                    other => kind
                        .cast_from(other)
                        .map(|typed| typed.send_binary())
                        .map_err(|_| invalid_data(codec.name)),
                },
            )
        }
    }
}

fn builtin_array_element_oid(array_oid: i64) -> Option<i64> {
    let array_oid = u32::try_from(array_oid).ok()?;
    crate::catalog::builtin_identity::builtin_array_element_oid(array_oid).map(i64::from)
}

fn binary_codec_for_oid(type_oid: i64) -> io::Result<&'static BinaryCodec> {
    u32::try_from(type_oid)
        .ok()
        .and_then(builtin_for_oid)
        .ok_or_else(|| unsupported_codec(type_oid))
}

fn validate_binary_oid(type_oid: i64) -> io::Result<()> {
    if type_oid == POSTGRES_INTERNAL_CHAR_OID {
        return Ok(());
    }
    if type_oid == 2278 {
        return Ok(());
    }
    if vector_dimensions_for_oid(type_oid).is_some() {
        return Ok(());
    }
    if (OID_ARRAY_BASE..OID_ARRAY_LIMIT).contains(&type_oid) {
        binary_codec_for_oid(type_oid - OID_ARRAY_BASE).map(|_| ())
    } else if let Some(element_oid) = builtin_array_element_oid(type_oid) {
        binary_codec_for_oid(element_oid).map(|_| ())
    } else {
        binary_codec_for_oid(type_oid).map(|_| ())
    }
}

pub(super) fn validate_result_formats(
    columns: &[ColumnMeta],
    result_formats: &[i16],
) -> io::Result<()> {
    if !result_formats.iter().all(|format| matches!(*format, 0 | 1)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported result format",
        ));
    }
    if !result_formats.is_empty()
        && result_formats.len() != 1
        && result_formats.len() != columns.len()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported result format count",
        ));
    }
    for (index, column) in columns.iter().enumerate() {
        if result_format_for_index(result_formats, index) == 1 {
            validate_binary_oid(column.type_oid)?;
        }
    }
    Ok(())
}

pub(super) fn binary_to_value(parameter: &[u8], type_oid: i64) -> io::Result<Value> {
    if type_oid == 4073 {
        return Err(unsupported_codec(type_oid));
    }
    if vector_dimensions_for_oid(type_oid).is_some() {
        return decode_vector(parameter, type_oid);
    }
    if (OID_ARRAY_BASE..OID_ARRAY_LIMIT).contains(&type_oid) {
        return decode_array(parameter, type_oid - OID_ARRAY_BASE);
    }
    if let Some(element_oid) = builtin_array_element_oid(type_oid) {
        return decode_array(parameter, element_oid);
    }
    let codec = binary_codec_for_oid(type_oid)?;
    let original_parameter = parameter;
    let parameter = usize::try_from(codec.typlen)
        .ok()
        .and_then(|width| parameter.get(..width))
        .unwrap_or(parameter);
    // Type receive functions validate their consumed prefix before PostgreSQL
    // checks for unread bytes. Preserve that error priority for temporal values.
    let value = match codec.kind {
        BinaryCodecKind::Bool => {
            fixed_bytes::<1>(parameter, codec.name).map(|bytes| Value::Bool(bytes[0] != 0))
        }
        BinaryCodecKind::Bytea => Ok(Value::String(hex_bytea(parameter))),
        BinaryCodecKind::Int2 => fixed_bytes::<2>(parameter, codec.name)
            .map(i16::from_be_bytes)
            .map(i64::from)
            .map(Value::Int64),
        BinaryCodecKind::Int4 => fixed_bytes::<4>(parameter, codec.name)
            .map(i32::from_be_bytes)
            .map(i64::from)
            .map(Value::Int64),
        BinaryCodecKind::Int8 => fixed_bytes::<8>(parameter, codec.name)
            .map(i64::from_be_bytes)
            .map(Value::Int64),
        BinaryCodecKind::ObjectId(_) => fixed_bytes::<4>(parameter, codec.name)
            .map(u32::from_be_bytes)
            .map(i64::from)
            .map(Value::Int64),
        BinaryCodecKind::Extended(kind) => {
            kind.receive_binary(parameter)
                .map(Value::Typed)
                .map_err(|message| {
                    // A short frame is a protocol violation (08P01); a
                    // complete frame with invalid content is 22P03.
                    if kind == crate::types::extended::ExtendedType::Real
                        || message == crate::types::extended::network::INSUFFICIENT_DATA
                    {
                        invalid_data(codec.name)
                    } else {
                        io::Error::new(io::ErrorKind::InvalidData, ExcessBinaryData(codec.name))
                    }
                })
        }
        BinaryCodecKind::Float8 => fixed_bytes::<8>(parameter, codec.name)
            .map(f64::from_be_bytes)
            .map(Value::Float64),
        BinaryCodecKind::Text
        | BinaryCodecKind::Bpchar
        | BinaryCodecKind::Varchar
        | BinaryCodecKind::Unknown => std::str::from_utf8(parameter)
            .map(|value| Value::String(value.to_string()))
            .map_err(|_| invalid_data(codec.name)),
        BinaryCodecKind::Jsonb => jsonb::decode(parameter),
        BinaryCodecKind::Json => {
            let value = std::str::from_utf8(parameter).map_err(|_| invalid_data(codec.name))?;
            serde_json::from_str(value)
                .map(Value::Json)
                .map_err(|_| invalid_data(codec.name))
        }
        BinaryCodecKind::JsonPath => {
            let Some((&version, path)) = parameter.split_first() else {
                return Err(invalid_data(codec.name));
            };
            if version != 1 {
                return Err(invalid_data(codec.name));
            }
            let path = std::str::from_utf8(path).map_err(|_| invalid_data(codec.name))?;
            sql_json_path::JsonPath::new(path)
                .map(|path| Value::String(path.to_string()))
                .map_err(|_| invalid_data(codec.name))
        }
        BinaryCodecKind::Uuid => uuid::Uuid::from_slice(parameter)
            .map(|value| Value::String(value.to_string()))
            .map_err(|_| invalid_data(codec.name)),
        BinaryCodecKind::Date => decode_native(parameter, codec, "date"),
        BinaryCodecKind::Time => decode_native(parameter, codec, "time"),
        BinaryCodecKind::Timestamp => decode_native(parameter, codec, "timestamp"),
    }?;
    if parameter.len() != original_parameter.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            ExcessBinaryData(codec.name),
        ));
    }
    Ok(value)
}

/// The native carrier of the existing `date`/`time`/`timestamp` codecs.
const fn native_temporal(kind: BinaryCodecKind) -> Option<crate::types::extended::ExtendedType> {
    use crate::types::extended::ExtendedType;
    match kind {
        BinaryCodecKind::Date => Some(ExtendedType::Date),
        BinaryCodecKind::Time => Some(ExtendedType::Time { precision: None }),
        BinaryCodecKind::Timestamp => Some(ExtendedType::Timestamp { precision: None }),
        _ => None,
    }
}

/// `date_recv`/`time_recv`/`timestamp_recv`, range-checked like PostgreSQL.
fn decode_native(
    parameter: &[u8],
    codec: &BinaryCodec,
    type_name: &'static str,
) -> io::Result<Value> {
    use crate::types::typed::TypedValue;
    let kind = native_temporal(codec.kind).ok_or_else(|| invalid_data(codec.name))?;
    let value = kind
        .receive_binary(parameter)
        .map_err(|_| invalid_data(codec.name))?;
    let valid = match &value {
        TypedValue::Date(date) => {
            matches!(*date, i32::MIN | i32::MAX)
                || crate::types::extended::temporal::calendar::is_valid_date(i64::from(*date))
        }
        TypedValue::Time(time) => (0..=86_400_000_000).contains(time),
        TypedValue::Timestamp(value) => {
            matches!(*value, i64::MIN | i64::MAX)
                || crate::types::extended::temporal::calendar::is_valid_timestamp(*value)
        }
        _ => true,
    };
    if !valid {
        return Err(temporal_out_of_range(type_name));
    }
    Ok(Value::Typed(value))
}

fn encode_array(value: &Value, type_oid: i64, element_oid: i64) -> io::Result<Vec<u8>> {
    let Value::Json(serde_json::Value::Array(values)) = value else {
        return invalid_value("array");
    };
    let codec = binary_codec_for_oid(element_oid)?;
    let has_null = values.iter().any(serde_json::Value::is_null);
    let mut encoded = Vec::new();
    // PostgreSQL writes an empty array as zero dimensions with no dimension
    // header: ndim, has_null and the element OID only.
    let dimensions = i32::from(!values.is_empty());
    encoded.extend_from_slice(&dimensions.to_be_bytes());
    encoded.extend_from_slice(&i32::from(has_null).to_be_bytes());
    encoded.extend_from_slice(
        &i32::try_from(element_oid)
            .map_err(|_| unsupported_codec(type_oid))?
            .to_be_bytes(),
    );
    if values.is_empty() {
        return Ok(encoded);
    }
    encoded.extend_from_slice(
        &i32::try_from(values.len())
            .map_err(|_| invalid_data("array"))?
            .to_be_bytes(),
    );
    encoded.extend_from_slice(&1_i32.to_be_bytes());
    for value in values {
        if value.is_null() {
            encoded.extend_from_slice(&(-1_i32).to_be_bytes());
            continue;
        }
        let element = encode_array_element(value, element_oid, codec)?;
        encoded.extend_from_slice(
            &i32::try_from(element.len())
                .map_err(|_| invalid_data("array"))?
                .to_be_bytes(),
        );
        encoded.extend_from_slice(&element);
    }
    Ok(encoded)
}

fn decode_array(parameter: &[u8], expected_element_oid: i64) -> io::Result<Value> {
    let mut cursor = 0;
    let dimensions = read_i32(parameter, &mut cursor, "array")?;
    let has_null = read_i32(parameter, &mut cursor, "array")?;
    let element_oid = i64::from(read_i32(parameter, &mut cursor, "array")?);
    if !matches!(dimensions, 0 | 1)
        || !matches!(has_null, 0 | 1)
        || element_oid != expected_element_oid
    {
        return Err(invalid_data("array"));
    }
    binary_codec_for_oid(element_oid)?;
    if dimensions == 0 {
        // The canonical empty array: no dimension header and no elements.
        if has_null != 0 || cursor != parameter.len() {
            return Err(invalid_data("array"));
        }
        return Ok(Value::Json(serde_json::Value::Array(Vec::new())));
    }
    let length = usize::try_from(read_i32(parameter, &mut cursor, "array")?)
        .map_err(|_| invalid_data("array"))?;
    if read_i32(parameter, &mut cursor, "array")? != 1 {
        return Err(invalid_data("array"));
    }
    let minimum_payload_bytes = length
        .checked_mul(std::mem::size_of::<i32>())
        .ok_or_else(|| invalid_data("array"))?;
    if parameter.len().saturating_sub(cursor) < minimum_payload_bytes {
        return Err(invalid_data("array"));
    }
    let mut values = Vec::with_capacity(length);
    let mut sql_null_frame = false;
    let mut document_null = false;
    for _ in 0..length {
        let element_length = read_i32(parameter, &mut cursor, "array")?;
        if element_length == -1 {
            sql_null_frame = true;
            values.push(serde_json::Value::Null);
            continue;
        }
        let element_length = usize::try_from(element_length).map_err(|_| invalid_data("array"))?;
        let end = cursor
            .checked_add(element_length)
            .ok_or_else(|| invalid_data("array"))?;
        let bytes = parameter
            .get(cursor..end)
            .ok_or_else(|| invalid_data("array"))?;
        cursor = end;
        let value = binary_to_value(bytes, element_oid)?;
        if matches!(element_oid, 114 | 3802)
            && matches!(value, Value::Json(serde_json::Value::Null))
        {
            document_null = true;
            continue;
        }
        values.push(value_to_json(value));
    }
    if cursor != parameter.len() || (has_null == 0 && sql_null_frame) {
        return Err(invalid_data("array"));
    }
    if document_null {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            crate::types::array::TextArrayError::UnsupportedJsonDocumentNull.to_string(),
        ));
    }
    Ok(Value::Json(serde_json::Value::Array(values)))
}

fn read_i32(parameter: &[u8], cursor: &mut usize, type_name: &str) -> io::Result<i32> {
    let end = cursor
        .checked_add(4)
        .ok_or_else(|| invalid_data(type_name))?;
    let bytes = parameter
        .get(*cursor..end)
        .ok_or_else(|| invalid_data(type_name))?;
    *cursor = end;
    Ok(i32::from_be_bytes(
        bytes.try_into().expect("four-byte integer"),
    ))
}

fn encode_array_element(
    value: &serde_json::Value,
    element_oid: i64,
    codec: &BinaryCodec,
) -> io::Result<Vec<u8>> {
    if matches!(codec.kind, BinaryCodecKind::Json | BinaryCodecKind::Jsonb) {
        return value_to_binary(&Value::Json(value.clone()), element_oid);
    }
    match value {
        serde_json::Value::Bool(value) => value_to_binary(&Value::Bool(*value), element_oid),
        serde_json::Value::Number(value) => {
            let value = value
                .as_i64()
                .map(Value::Int64)
                .or_else(|| value.as_f64().map(Value::Float64))
                .ok_or_else(|| invalid_data(codec.name))?;
            value_to_binary(&value, element_oid)
        }
        serde_json::Value::String(value) => encode_array_string(value, codec),
        serde_json::Value::Object(_)
            if matches!(
                codec.kind,
                BinaryCodecKind::Text
                    | BinaryCodecKind::Bpchar
                    | BinaryCodecKind::Varchar
                    | BinaryCodecKind::Unknown
            ) =>
        {
            Ok(value.to_string().into_bytes())
        }
        _ => Err(invalid_data(codec.name)),
    }
}

fn encode_array_string(value: &str, codec: &BinaryCodec) -> io::Result<Vec<u8>> {
    match codec.kind {
        BinaryCodecKind::Bool => parse_bool_binary(value),
        BinaryCodecKind::Bytea => decode_bytea(value),
        BinaryCodecKind::Int2 => parse_i64_binary(value).and_then(encode_i16_binary),
        BinaryCodecKind::Int4 => parse_i64_binary(value).and_then(encode_i32_binary),
        BinaryCodecKind::Int8 => parse_i64_binary(value).map(|value| value.to_be_bytes().to_vec()),
        BinaryCodecKind::ObjectId(_) => parse_i64_binary(value).and_then(encode_oid_binary),
        BinaryCodecKind::Extended(kind) => kind
            .parse_text(value)
            .map(|typed| typed.send_binary())
            .map_err(|_| invalid_data(codec.name)),
        BinaryCodecKind::Float8 => {
            parse_f64_binary(value).map(|value| value.to_be_bytes().to_vec())
        }
        BinaryCodecKind::Text
        | BinaryCodecKind::Bpchar
        | BinaryCodecKind::Varchar
        | BinaryCodecKind::Unknown
        | BinaryCodecKind::JsonPath => Ok(value.as_bytes().to_vec()),
        BinaryCodecKind::Uuid => uuid::Uuid::parse_str(value)
            .map(|value| value.into_bytes().to_vec())
            .map_err(|_| invalid_data(codec.name)),
        BinaryCodecKind::Date => encode_date(value).map(|value| value.to_be_bytes().to_vec()),
        BinaryCodecKind::Time => encode_time(value).map(|value| value.to_be_bytes().to_vec()),
        BinaryCodecKind::Timestamp => {
            encode_timestamp(value).map(|value| value.to_be_bytes().to_vec())
        }
        BinaryCodecKind::Json | BinaryCodecKind::Jsonb => {
            unreachable!("JSON array elements preserve their JSON carrier")
        }
    }
}

fn value_to_json(value: Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(value) => serde_json::Value::Bool(value),
        Value::Int64(value) => serde_json::Value::from(value),
        Value::Float64(value) => serde_json::Value::from(value),
        Value::String(value) => serde_json::Value::String(value),
        Value::Json(value) => value,
        Value::Typed(value) => value.to_json(),
        Value::Vector(vector) => serde_json::Value::Array(
            vector
                .values
                .into_iter()
                .map(serde_json::Value::from)
                .collect(),
        ),
    }
}

fn result_format_for_index(formats: &[i16], index: usize) -> i16 {
    match formats.len() {
        0 => 0,
        1 => formats[0],
        _ => formats[index],
    }
}

fn encode_integer(
    value: &Value,
    type_name: &str,
    encode: fn(i64) -> io::Result<Vec<u8>>,
) -> io::Result<Vec<u8>> {
    match value {
        Value::Int64(value) => encode(*value),
        Value::String(value) => parse_i64_binary(value).and_then(encode),
        _ => invalid_value(type_name),
    }
}

fn encode_int8(value: &Value, type_name: &str) -> io::Result<Vec<u8>> {
    match value {
        Value::Int64(value) => Ok(value.to_be_bytes().to_vec()),
        Value::Float64(value) => encode_f64_as_i64_binary(*value),
        Value::String(value) => parse_i64_binary(value).map(|value| value.to_be_bytes().to_vec()),
        _ => invalid_value(type_name),
    }
}

fn encode_float8(value: &Value, type_name: &str) -> io::Result<Vec<u8>> {
    match value {
        Value::Float64(value) => Ok(value.to_be_bytes().to_vec()),
        Value::Int64(value) => encode_i64_as_f64_binary(*value),
        Value::String(value) => parse_f64_binary(value).map(|value| value.to_be_bytes().to_vec()),
        _ => invalid_value(type_name),
    }
}

fn fixed_bytes<const N: usize>(value: &[u8], type_name: &str) -> io::Result<[u8; N]> {
    value.try_into().map_err(|_| invalid_data(type_name))
}

fn unsupported_codec(type_oid: i64) -> io::Error {
    let family = if vector_dimensions_for_oid(type_oid).is_some() {
        "vector"
    } else if (OID_ARRAY_BASE..OID_ARRAY_LIMIT).contains(&type_oid) {
        "array"
    } else {
        "type"
    };
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("binary format is not supported for {family} OID {type_oid}"),
    )
}

fn invalid_data(type_name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("invalid binary {type_name} value"),
    )
}

fn invalid_value(type_name: &str) -> io::Result<Vec<u8>> {
    Err(invalid_data(type_name))
}

fn encode_i16_binary(value: i64) -> io::Result<Vec<u8>> {
    i16::try_from(value)
        .map(|value| value.to_be_bytes().to_vec())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "int2 out of range"))
}

fn encode_i32_binary(value: i64) -> io::Result<Vec<u8>> {
    i32::try_from(value)
        .map(|value| value.to_be_bytes().to_vec())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "int4 out of range"))
}

fn encode_oid_binary(value: i64) -> io::Result<Vec<u8>> {
    u32::try_from(value)
        .map(|value| value.to_be_bytes().to_vec())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "oid out of range"))
}

fn encode_f64_as_i64_binary(value: f64) -> io::Result<Vec<u8>> {
    let integer = parse_f64_to_i64(value)?;
    Ok(integer.to_be_bytes().to_vec())
}

fn encode_i64_as_f64_binary(value: i64) -> io::Result<Vec<u8>> {
    parse_i64_to_f64(value).map(|value| value.to_be_bytes().to_vec())
}

fn parse_bool_binary(value: &str) -> io::Result<Vec<u8>> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "t" | "1" => Ok(vec![1]),
        "false" | "f" | "0" => Ok(vec![0]),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cannot encode bool value to binary",
        )),
    }
}

fn parse_i64_binary(value: &str) -> io::Result<i64> {
    value.parse::<i64>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "cannot encode integer value to binary",
        )
    })
}

fn parse_f64_binary(value: &str) -> io::Result<f64> {
    value.parse::<f64>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "cannot encode float value to binary",
        )
    })
}

fn parse_f64_to_i64(value: f64) -> io::Result<i64> {
    if !value.is_finite() || value.fract() != 0.0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cannot encode non-integral float as int8",
        ));
    }

    format!("{value:.0}").parse::<i64>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "cannot encode float value to int8",
        )
    })
}

fn parse_i64_to_f64(value: i64) -> io::Result<f64> {
    crate::types::numeric::exact_i64_to_f64(value).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "cannot encode int8 value to float8",
        )
    })
}

fn encode_date(value: &str) -> io::Result<i32> {
    let date = parse_date(value)?;
    date.to_julian_day()
        .checked_sub(POSTGRES_EPOCH_JULIAN_DAY)
        .ok_or_else(|| invalid_data("date"))
}

fn encode_time(value: &str) -> io::Result<i64> {
    let time = parse_time(value)?;
    let micros = i64::from(time.hour()) * 3_600_000_000
        + i64::from(time.minute()) * 60_000_000
        + i64::from(time.second()) * 1_000_000
        + i64::from(time.microsecond());
    Ok(micros)
}

fn encode_timestamp(value: &str) -> io::Result<i64> {
    let datetime = parse_timestamp(value)?;
    let days = i64::from(
        datetime
            .date()
            .to_julian_day()
            .checked_sub(POSTGRES_EPOCH_JULIAN_DAY)
            .ok_or_else(|| invalid_data("timestamp"))?,
    );
    let time = encode_time(&format_time(datetime.time()))?;
    days.checked_mul(MICROSECONDS_PER_DAY)
        .and_then(|days| days.checked_add(time))
        .ok_or_else(|| invalid_data("timestamp"))
}

// DATE/TIME/TIMESTAMP parsing and canonical formatting are shared with
// storage and casts via `crate::types::temporal`; these just adapt that
// shared parser's `Result<_, String>` to this module's `io::Result`.
fn parse_date(value: &str) -> io::Result<Date> {
    crate::types::temporal::parse_date(value).map_err(|_| invalid_data("date"))
}

fn parse_time(value: &str) -> io::Result<Time> {
    crate::types::temporal::parse_time(value).map_err(|_| invalid_data("time"))
}

fn parse_timestamp(value: &str) -> io::Result<PrimitiveDateTime> {
    crate::types::temporal::parse_timestamp(value).map_err(|_| invalid_data("timestamp"))
}

fn format_time(time: Time) -> String {
    crate::types::temporal::format_time(time)
}

pub(super) fn hex_bytea(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(2 + bytes.len().saturating_mul(2));
    out.push_str("\\x");
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

pub(super) fn decode_bytea(value: &str) -> io::Result<Vec<u8>> {
    if !value.starts_with("\\x") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bytea value must be hex format '\\x...'",
        ));
    }
    if value.len() == 2 {
        return Ok(Vec::new());
    }
    if (value.len() - 2).rem_euclid(2) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bytea value must have an even number of hex digits",
        ));
    }

    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity((value.len() - 2) / 2);
    let mut index = 2;
    while index < value.len() {
        let high = decode_hex_digit(bytes[index]).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "bytea value must be hexadecimal",
            )
        })?;
        let low = decode_hex_digit(bytes[index + 1]).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "bytea value must be hexadecimal",
            )
        })?;
        out.push((high << 4) | low);
        index += 2;
    }

    Ok(out)
}

pub(super) fn decode_hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[derive(Debug, thiserror::Error)]
#[error("incorrect binary data format for {0}")]
struct ExcessBinaryData(&'static str);

#[derive(Debug, thiserror::Error)]
#[error("{0} out of range")]
struct DateTimeOutOfRange(&'static str);

fn temporal_out_of_range(type_name: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, DateTimeOutOfRange(type_name))
}

pub(super) fn binary_error_sqlstate(error: &io::Error) -> Option<&'static str> {
    let error = error.get_ref()?;
    if let Some(error) = error.downcast_ref::<crate::types::extended::TypeInputError>() {
        Some(error.sqlstate)
    } else if error.is::<ExcessBinaryData>() {
        Some("22P03")
    } else if error.is::<DateTimeOutOfRange>() {
        Some("22008")
    } else {
        None
    }
}
