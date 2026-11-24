use std::collections::HashSet;

use crate::app::CassieError;
use crate::types::{ArrayDimension, DataType, FieldSchema, Schema, SqlArray, Value};
use uuid::Uuid;

pub(crate) use cassie_storage::row_blob::{
    Cursor, RowBlobEncodeError, RowBlobField, RowBlobRecord, RowFieldMeta, RowSchema,
};
pub(crate) use cassie_storage::row_blob::{
    FORMAT_VERSION, MAGIC, TYPE_ARRAY, TYPE_BOOL, TYPE_BYTEA, TYPE_COMPOSITE, TYPE_DATE,
    TYPE_DOMAIN, TYPE_ENUM, TYPE_EXTENDED, TYPE_F64, TYPE_I64, TYPE_JSON, TYPE_NULL, TYPE_STRING,
    TYPE_TIME, TYPE_TIMESTAMP, TYPE_UUID, TYPE_VECTOR_F32, field_bit,
};

mod compact;
mod composite;
mod domain;
mod encoding;
mod enum_value;
mod vector_field;

use self::compact::{decode_directory_value, expected_type_tag};
pub(crate) use compact::{
    CompactScalar, decode_compact_field_value, decode_compact_scalar, encode_compact_sql_null,
    encode_compact_value,
};
pub(crate) use vector_field::{PackedVector, VectorColumn, split_vector_row};

pub(crate) fn encode_row(
    schema: &RowSchema,
    payload: &serde_json::Value,
) -> Result<Vec<u8>, CassieError> {
    cassie_storage::row_blob::encode_row(schema, payload, encoding::encode_value).map_err(|error| {
        match error {
            RowBlobEncodeError::Layout(error) => CassieError::from(error),
            RowBlobEncodeError::FieldValue(error) => error,
        }
    })
}

pub(crate) fn decode_row(schema: &RowSchema, row: &[u8]) -> Result<serde_json::Value, CassieError> {
    decode_row_with_projection(schema, row, None, false)
}

pub(crate) fn decode_projected_row(
    schema: &RowSchema,
    row: &[u8],
    projection: &HashSet<String>,
) -> Result<serde_json::Value, CassieError> {
    decode_row_with_projection(schema, row, Some(projection), false)
}

pub(crate) fn decode_projected_row_with_aliases(
    schema: &RowSchema,
    row: &[u8],
    projection: &HashSet<String>,
) -> Result<serde_json::Value, CassieError> {
    decode_row_with_projection(schema, row, Some(projection), true)
}

#[cfg(test)]
pub(crate) fn decode_projected_row_matching(
    schema: &RowSchema,
    row: &[u8],
    projection: &HashSet<String>,
    filter_field: &str,
    filter_value: &serde_json::Value,
) -> Result<Option<serde_json::Value>, CassieError> {
    decode_projected_row_matching_with_aliases(
        schema,
        row,
        projection,
        filter_field,
        filter_value,
        false,
    )
}

pub(crate) fn decode_projected_row_matching_with_aliases(
    schema: &RowSchema,
    row: &[u8],
    projection: &HashSet<String>,
    filter_field: &str,
    filter_value: &serde_json::Value,
    include_historical_aliases: bool,
) -> Result<Option<serde_json::Value>, CassieError> {
    let directory = RowDirectory::parse(row)?;
    let filter_field = crate::sql::ColumnIdentifierPath::parse(filter_field).map_or_else(
        |_| filter_field.to_ascii_lowercase(),
        |field| field.field_lookup_key(),
    );
    let mut object = serde_json::Map::new();
    let mut matched_filter = false;
    let mut saw_filter = false;
    let active_field_names = active_field_names(schema);

    for field in &schema.fields {
        let include_names = included_field_names(
            field,
            Some(projection),
            include_historical_aliases,
            &active_field_names,
        );
        let is_filter_field = field_matches_name(
            field,
            &filter_field,
            include_historical_aliases,
            &active_field_names,
        );
        if !include_names.is_empty() || is_filter_field {
            let Some(value) = directory.decode(field)? else {
                continue;
            };
            if is_filter_field {
                saw_filter = true;
                matched_filter = value_matches_filter(&value, filter_value);
            }
            for name in include_names {
                object.insert(name, value.clone());
            }
        }
    }

    Ok((saw_filter && matched_filter).then_some(serde_json::Value::Object(object)))
}

fn decode_row_with_projection(
    schema: &RowSchema,
    row: &[u8],
    projection: Option<&HashSet<String>>,
    include_historical_aliases: bool,
) -> Result<serde_json::Value, CassieError> {
    let directory = RowDirectory::parse(row)?;
    let mut object = serde_json::Map::new();
    let active_field_names = active_field_names(schema);

    for field in &schema.fields {
        let include_names = included_field_names(
            field,
            projection,
            include_historical_aliases,
            &active_field_names,
        );
        if !include_names.is_empty() {
            let Some(value) = directory.decode(field)? else {
                continue;
            };
            for name in include_names {
                object.insert(name, value.clone());
            }
        }
    }

    Ok(serde_json::Value::Object(object))
}

struct RowDirectory<'a>(RowBlobRecord<'a>);

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

fn value_matches_filter(value: &serde_json::Value, filter_value: &serde_json::Value) -> bool {
    value == filter_value
}

fn included_field_names(
    field: &RowFieldMeta,
    projection: Option<&HashSet<String>>,
    include_historical_aliases: bool,
    active_field_names: &HashSet<String>,
) -> Vec<String> {
    let Some(projection) = projection else {
        return (!field.retired)
            .then(|| field.name.clone())
            .into_iter()
            .collect();
    };

    let mut names = Vec::new();
    let field_key = crate::sql::ColumnIdentifierPath::from_field_name(&field.name).lookup_key();
    if !field.retired && projection.contains(&field_key) {
        names.push(field.name.clone());
    }
    if include_historical_aliases {
        for alias in &field.aliases {
            let alias_key = crate::sql::ColumnIdentifierPath::from_field_name(alias).lookup_key();
            if projection.contains(&alias_key)
                && !active_field_names.contains(&alias_key)
                && !names.iter().any(|name| name == alias)
            {
                names.push(alias.clone());
            }
        }
    }
    names
}

fn field_matches_name(
    field: &RowFieldMeta,
    requested_key: &str,
    include_historical_aliases: bool,
    active_field_names: &HashSet<String>,
) -> bool {
    (!field.retired
        && crate::sql::ColumnIdentifierPath::from_field_name(&field.name).lookup_key()
            == requested_key)
        || (include_historical_aliases
            && field.aliases.iter().any(|alias| {
                crate::sql::ColumnIdentifierPath::from_field_name(alias).lookup_key()
                    == requested_key
            })
            && !active_field_names.contains(requested_key))
}

fn active_field_names(schema: &RowSchema) -> HashSet<String> {
    schema
        .fields
        .iter()
        .filter(|field| !field.retired)
        .map(|field| crate::sql::ColumnIdentifierPath::from_field_name(&field.name).lookup_key())
        .collect()
}

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

fn write_varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        let byte = u8::try_from(value & 0x7f).expect("masked to 7 bits");
        out.push(byte | 0x80);
        value >>= 7;
    }
    out.push(u8::try_from(value).expect("final varint byte fits in u8"));
}

pub(crate) fn canonical_bytea_text(value: &str) -> Result<String, CassieError> {
    decode_bytea(value).map(|bytes| encode_bytea(&bytes))
}

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

fn decode_hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

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

#[cfg(test)]
#[path = "row_blob/tests.rs"]
pub(crate) mod tests;
