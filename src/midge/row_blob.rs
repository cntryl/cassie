use std::collections::HashSet;

use crate::app::CassieError;
use crate::types::{DataType, FieldSchema, Schema};
use uuid::Uuid;

const MAGIC: &[u8; 4] = b"CRB1";
const FORMAT_VERSION: u8 = 1;
const TYPE_NULL: u8 = 0x00;
const TYPE_BOOL: u8 = 0x01;
const TYPE_I64: u8 = 0x02;
const TYPE_F64: u8 = 0x04;
const TYPE_STRING: u8 = 0x05;
const TYPE_UUID: u8 = 0x06;
const TYPE_JSON: u8 = 0x07;
const TYPE_VECTOR_F32: u8 = 0x08;
const TYPE_DATE: u8 = 0x09;
const TYPE_TIME: u8 = 0x0A;
const TYPE_TIMESTAMP: u8 = 0x0B;
const TYPE_ARRAY: u8 = 0x0C;
const TYPE_BYTEA: u8 = 0x0D;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct RowSchema {
    pub schema_version: u32,
    pub next_field_id: u32,
    pub fields: Vec<RowFieldMeta>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct RowFieldMeta {
    pub field_id: u32,
    pub name: String,
    #[serde(default)]
    pub normalized_name: String,
    pub data_type: DataType,
    pub nullable: bool,
    pub retired: bool,
}

impl RowSchema {
    pub(crate) fn from_schema(schema: &Schema) -> Self {
        let mut fields = schema
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| RowFieldMeta {
                field_id: (index + 1) as u32,
                name: field.name.clone(),
                normalized_name: field.name.to_ascii_lowercase(),
                data_type: field.data_type.clone(),
                nullable: field.nullable,
                retired: false,
            })
            .collect::<Vec<_>>();
        hydrate_normalized_names(fields.as_mut_slice());

        Self {
            schema_version: 1,
            next_field_id: fields.len() as u32 + 1,
            fields,
        }
    }

    pub(crate) fn active_schema(&self) -> Schema {
        Schema {
            fields: self
                .fields
                .iter()
                .filter(|field| !field.retired)
                .map(|field| FieldSchema {
                    name: field.name.clone(),
                    data_type: field.data_type.clone(),
                    nullable: field.nullable,
                })
                .collect(),
        }
    }

    pub(crate) fn add_field(&mut self, field: FieldSchema) -> Result<(), CassieError> {
        if self
            .fields
            .iter()
            .any(|entry| entry.name.eq_ignore_ascii_case(&field.name) && !entry.retired)
        {
            return Err(CassieError::Unsupported(format!(
                "field '{0}' already exists",
                field.name
            )));
        }

        let normalized_name = field.name.to_ascii_lowercase();
        self.fields.push(RowFieldMeta {
            field_id: self.next_field_id,
            name: field.name,
            normalized_name,
            data_type: field.data_type,
            nullable: field.nullable,
            retired: false,
        });
        self.next_field_id += 1;
        self.schema_version += 1;
        Ok(())
    }

    pub(crate) fn rename_field(&mut self, current: &str, next: &str) -> Result<(), CassieError> {
        if self
            .fields
            .iter()
            .any(|entry| entry.name.eq_ignore_ascii_case(next) && !entry.retired)
        {
            return Err(CassieError::Unsupported(format!(
                "field '{next}' already exists"
            )));
        }

        let Some(field) = self
            .fields
            .iter_mut()
            .find(|entry| entry.name.eq_ignore_ascii_case(current) && !entry.retired)
        else {
            return Err(CassieError::Unsupported(format!(
                "field '{current}' not found"
            )));
        };

        field.name = next.to_string();
        field.normalized_name = next.to_ascii_lowercase();
        self.schema_version += 1;
        Ok(())
    }

    pub(crate) fn retire_field(&mut self, name: &str) -> bool {
        let Some(field) = self
            .fields
            .iter_mut()
            .find(|entry| entry.name == name && !entry.retired)
        else {
            return false;
        };

        field.retired = true;
        self.schema_version += 1;
        true
    }

    fn active_fields_by_id(&self) -> Vec<&RowFieldMeta> {
        self.fields.iter().filter(|field| !field.retired).collect()
    }

    fn field_by_id(&self, field_id: u32) -> Option<&RowFieldMeta> {
        let index = usize::try_from(field_id.checked_sub(1)?).ok()?;
        self.fields
            .get(index)
            .filter(|field| field.field_id == field_id)
    }

    fn active_field_by_name(&self, name: &str) -> Option<&RowFieldMeta> {
        let normalized = name.to_ascii_lowercase();
        self.fields
            .iter()
            .find(|field| !field.retired && field.normalized_name == normalized)
    }
}

fn hydrate_normalized_names(fields: &mut [RowFieldMeta]) {
    for field in fields {
        if field.normalized_name.is_empty() {
            field.normalized_name = field.name.to_ascii_lowercase();
        }
    }
}

pub(crate) fn encode_row(
    schema: &RowSchema,
    payload: &serde_json::Value,
) -> Result<Vec<u8>, CassieError> {
    let object = payload
        .as_object()
        .ok_or_else(|| CassieError::InvalidVector("document must be object".to_string()))?;

    let mut fields = Vec::new();
    for field in schema.active_fields_by_id() {
        let Some(value) = object.get(&field.name) else {
            continue;
        };
        let (type_tag, encoded) = encode_value(&field.data_type, value)?;
        fields.push((field.field_id, type_tag, encoded));
    }

    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.push(FORMAT_VERSION);
    out.extend_from_slice(&schema.schema_version.to_be_bytes());
    out.push(0);
    write_varint(fields.len() as u64, &mut out);

    for (field_id, type_tag, encoded) in fields {
        write_field_value(field_id, type_tag, &encoded, &mut out)?;
    }

    Ok(out)
}

fn write_field_value(
    field_id: u32,
    type_tag: u8,
    encoded: &[u8],
    out: &mut Vec<u8>,
) -> Result<(), CassieError> {
    write_varint(field_id as u64, out);
    out.push(type_tag);

    match type_tag {
        TYPE_NULL => {}
        TYPE_BOOL | TYPE_I64 | TYPE_F64 | TYPE_UUID | TYPE_ARRAY => out.extend_from_slice(encoded),
        TYPE_STRING | TYPE_JSON | TYPE_VECTOR_F32 | TYPE_DATE | TYPE_TIME | TYPE_TIMESTAMP => {
            write_varint(encoded.len() as u64, out);
            out.extend_from_slice(encoded);
        }
        TYPE_BYTEA => {
            write_varint(encoded.len() as u64, out);
            out.extend_from_slice(encoded);
        }
        _ => {
            return Err(CassieError::Parse(format!(
                "unsupported row blob type tag {type_tag}"
            )));
        }
    }

    Ok(())
}

pub(crate) fn decode_row(schema: &RowSchema, row: &[u8]) -> Result<serde_json::Value, CassieError> {
    decode_row_with_projection(schema, row, None)
}

pub(crate) fn decode_projected_row(
    schema: &RowSchema,
    row: &[u8],
    projection: &HashSet<String>,
) -> Result<serde_json::Value, CassieError> {
    decode_row_with_projection(schema, row, Some(projection))
}

pub(crate) fn decode_projected_row_matching(
    schema: &RowSchema,
    row: &[u8],
    projection: &HashSet<String>,
    filter_field: &str,
    filter_value: &serde_json::Value,
) -> Result<Option<serde_json::Value>, CassieError> {
    if row.first() == Some(&b'{') {
        let payload: serde_json::Value = serde_json::from_slice(row).map_err(|error| {
            CassieError::Parse(format!("invalid legacy JSON document: {error}"))
        })?;
        if !json_object_matches_filter(schema, &payload, filter_field, filter_value)? {
            return Ok(None);
        }
        return filter_json_object(schema, payload, Some(projection)).map(Some);
    }

    let mut cursor = Cursor::new(row);
    cursor.expect_bytes(MAGIC)?;
    let version = cursor.read_u8()?;
    if version != FORMAT_VERSION {
        return Err(CassieError::Parse(format!(
            "unsupported row blob format version {version}"
        )));
    }

    let _schema_version = cursor.read_u32()?;
    let _flags = cursor.read_u8()?;
    let field_count = cursor.read_varint()?;
    let filter_field = filter_field.to_ascii_lowercase();
    let mut object = serde_json::Map::new();
    let mut matched_filter = false;
    let mut saw_filter = false;

    for _ in 0..field_count {
        let field_id = cursor.read_varint()? as u32;
        let type_tag = cursor.read_u8()?;
        let Some(field) = schema.field_by_id(field_id) else {
            skip_value(type_tag, &mut cursor)?;
            continue;
        };

        let include_field = should_include_field(field, Some(projection));
        let is_filter_field = !field.retired && field.normalized_name == filter_field;
        if include_field || is_filter_field {
            let value = decode_value(type_tag, &mut cursor)?;
            if is_filter_field {
                saw_filter = true;
                matched_filter = value_matches_filter(&value, filter_value);
            }
            if include_field {
                object.insert(field.name.clone(), value);
            }
        } else {
            skip_value(type_tag, &mut cursor)?;
        }
    }

    Ok((saw_filter && matched_filter).then_some(serde_json::Value::Object(object)))
}

fn decode_row_with_projection(
    schema: &RowSchema,
    row: &[u8],
    projection: Option<&HashSet<String>>,
) -> Result<serde_json::Value, CassieError> {
    if row.first() == Some(&b'{') {
        let payload: serde_json::Value = serde_json::from_slice(row).map_err(|error| {
            CassieError::Parse(format!("invalid legacy JSON document: {error}"))
        })?;
        return filter_json_object(schema, payload, projection);
    }

    let mut cursor = Cursor::new(row);
    cursor.expect_bytes(MAGIC)?;
    let version = cursor.read_u8()?;
    if version != FORMAT_VERSION {
        return Err(CassieError::Parse(format!(
            "unsupported row blob format version {version}"
        )));
    }

    let _schema_version = cursor.read_u32()?;
    let _flags = cursor.read_u8()?;
    let field_count = cursor.read_varint()?;
    let mut object = serde_json::Map::new();

    for _ in 0..field_count {
        let field_id = cursor.read_varint()? as u32;
        let type_tag = cursor.read_u8()?;
        let Some(field) = schema.field_by_id(field_id) else {
            skip_value(type_tag, &mut cursor)?;
            continue;
        };
        if should_include_field(field, projection) {
            let value = decode_value(type_tag, &mut cursor)?;
            object.insert(field.name.clone(), value);
        } else {
            skip_value(type_tag, &mut cursor)?;
        }
    }

    Ok(serde_json::Value::Object(object))
}

fn filter_json_object(
    schema: &RowSchema,
    payload: serde_json::Value,
    projection: Option<&HashSet<String>>,
) -> Result<serde_json::Value, CassieError> {
    let object = payload
        .as_object()
        .ok_or_else(|| CassieError::InvalidVector("document must be object".to_string()))?;
    let mut out = serde_json::Map::new();

    match projection {
        Some(projection) => {
            for field_name in projection {
                if let Some(field) = schema.active_field_by_name(field_name) {
                    if let Some(value) = object.get(&field.name) {
                        out.insert(field.name.clone(), value.clone());
                    }
                }
            }
        }
        None => {
            for field in schema.active_fields_by_id() {
                if let Some(value) = object.get(&field.name) {
                    out.insert(field.name.clone(), value.clone());
                }
            }
        }
    }

    Ok(serde_json::Value::Object(out))
}

fn json_object_matches_filter(
    schema: &RowSchema,
    payload: &serde_json::Value,
    filter_field: &str,
    filter_value: &serde_json::Value,
) -> Result<bool, CassieError> {
    let object = payload
        .as_object()
        .ok_or_else(|| CassieError::InvalidVector("document must be object".to_string()))?;
    let Some(field) = schema.active_field_by_name(filter_field) else {
        return Ok(false);
    };
    Ok(object
        .get(&field.name)
        .is_some_and(|value| value_matches_filter(value, filter_value)))
}

fn value_matches_filter(value: &serde_json::Value, filter_value: &serde_json::Value) -> bool {
    value == filter_value
}

fn should_include_field(field: &RowFieldMeta, projection: Option<&HashSet<String>>) -> bool {
    if field.retired {
        return false;
    }

    projection.is_none_or(|projection| projection.contains(&field.normalized_name))
}

fn skip_value(type_tag: u8, cursor: &mut Cursor<'_>) -> Result<(), CassieError> {
    match type_tag {
        TYPE_NULL => Ok(()),
        TYPE_BOOL => cursor.skip_exact(1),
        TYPE_I64 | TYPE_F64 => cursor.skip_exact(8),
        TYPE_UUID => cursor.skip_exact(16),
        TYPE_STRING | TYPE_JSON | TYPE_VECTOR_F32 | TYPE_DATE | TYPE_TIME | TYPE_TIMESTAMP
        | TYPE_BYTEA => cursor.skip_len_prefixed(),
        TYPE_ARRAY => {
            let count = cursor.read_varint()? as usize;
            for _ in 0..count {
                let value_type = cursor.read_u8()?;
                skip_array_value(value_type, cursor)?;
            }
            Ok(())
        }
        _ => Err(CassieError::Parse(format!(
            "unsupported row blob type tag {type_tag}"
        ))),
    }
}

fn skip_array_value(type_tag: u8, cursor: &mut Cursor<'_>) -> Result<(), CassieError> {
    match type_tag {
        TYPE_BOOL => cursor.skip_exact(1),
        TYPE_I64 | TYPE_F64 => cursor.skip_exact(8),
        TYPE_UUID => cursor.skip_exact(16),
        TYPE_NULL | TYPE_STRING | TYPE_JSON | TYPE_VECTOR_F32 | TYPE_DATE | TYPE_TIME
        | TYPE_TIMESTAMP | TYPE_ARRAY | TYPE_BYTEA => cursor.skip_len_prefixed(),
        _ => Err(CassieError::Parse(format!(
            "unsupported row blob array type tag {type_tag}"
        ))),
    }
}

fn encode_value(
    data_type: &DataType,
    value: &serde_json::Value,
) -> Result<(u8, Vec<u8>), CassieError> {
    if value.is_null() {
        return Ok((TYPE_NULL, Vec::new()));
    }

    match data_type {
        DataType::Null => Ok((TYPE_NULL, Vec::new())),
        DataType::SmallInt => {
            let value = value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
                .and_then(|value| i16::try_from(value).ok())
                .ok_or_else(|| CassieError::InvalidVector("smallint field expects i16".into()))?;
            Ok((TYPE_I64, i64::from(value).to_be_bytes().to_vec()))
        }
        DataType::Int => {
            let value = value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
                .and_then(|value| i32::try_from(value).ok())
                .ok_or_else(|| CassieError::InvalidVector("integer field expects i32".into()))?;
            Ok((TYPE_I64, i64::from(value).to_be_bytes().to_vec()))
        }
        DataType::BigInt => {
            let value = value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
                .ok_or_else(|| CassieError::InvalidVector("bigint field expects i64".into()))?;
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
        DataType::Text => {
            let value = value
                .as_str()
                .ok_or_else(|| CassieError::InvalidVector("text field expects string".into()))?;
            Ok((TYPE_STRING, value.as_bytes().to_vec()))
        }
        DataType::Char { length } => {
            let value = value
                .as_str()
                .ok_or_else(|| CassieError::InvalidVector("char field expects string".into()))?;
            let length = length.unwrap_or(1);
            if value.chars().count() > length as usize {
                return Err(CassieError::InvalidVector(format!(
                    "char field expects up to {length} characters"
                )));
            }
            Ok((TYPE_STRING, value.as_bytes().to_vec()))
        }
        DataType::Varchar { length } => {
            let value = value
                .as_str()
                .ok_or_else(|| CassieError::InvalidVector("varchar field expects string".into()))?;
            if let Some(length) = length {
                if value.chars().count() > *length as usize {
                    return Err(CassieError::InvalidVector(format!(
                        "varchar field expects up to {length} characters"
                    )));
                }
            }
            Ok((TYPE_STRING, value.as_bytes().to_vec()))
        }
        DataType::Uuid => {
            let value = value
                .as_str()
                .ok_or_else(|| CassieError::InvalidVector("uuid field expects string".into()))?;
            let uuid = Uuid::parse_str(value)
                .map_err(|_| CassieError::InvalidVector("uuid field expects UUID".into()))?;
            Ok((TYPE_UUID, uuid.as_bytes().to_vec()))
        }
        DataType::Date => {
            let value = value
                .as_str()
                .ok_or_else(|| CassieError::InvalidVector("date field expects string".into()))?;
            Ok((TYPE_DATE, value.as_bytes().to_vec()))
        }
        DataType::Time => {
            let value = value
                .as_str()
                .ok_or_else(|| CassieError::InvalidVector("time field expects string".into()))?;
            Ok((TYPE_TIME, value.as_bytes().to_vec()))
        }
        DataType::Timestamp => {
            let value = value.as_str().ok_or_else(|| {
                CassieError::InvalidVector("timestamp field expects string".into())
            })?;
            Ok((TYPE_TIMESTAMP, value.as_bytes().to_vec()))
        }
        DataType::Bytea => {
            let value = value
                .as_str()
                .ok_or_else(|| CassieError::InvalidVector("bytea field expects string".into()))?;
            let decoded = decode_bytea(value)?;
            Ok((TYPE_BYTEA, decoded))
        }
        DataType::Array(inner) => {
            let values = value
                .as_array()
                .ok_or_else(|| CassieError::InvalidVector("array field expects array".into()))?;

            let mut out = Vec::new();
            write_varint(values.len() as u64, &mut out);
            for value in values {
                let (value_type, value_data) = encode_value(inner, value)?;
                out.push(value_type);
                match value_type {
                    TYPE_BOOL | TYPE_I64 | TYPE_F64 | TYPE_UUID => {
                        out.extend_from_slice(&value_data)
                    }
                    TYPE_NULL | TYPE_STRING | TYPE_JSON | TYPE_VECTOR_F32 | TYPE_DATE
                    | TYPE_TIME | TYPE_TIMESTAMP | TYPE_ARRAY | TYPE_BYTEA => {
                        write_varint(value_data.len() as u64, &mut out);
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
        DataType::Json => {
            let encoded = serde_json::to_vec(value)
                .map_err(|error| CassieError::Parse(format!("invalid json field: {error}")))?;
            Ok((TYPE_JSON, encoded))
        }
        DataType::Vector(dimensions) => {
            let values = value
                .as_array()
                .ok_or_else(|| CassieError::InvalidVector("vector field expects array".into()))?;
            if values.len() != *dimensions {
                return Err(CassieError::InvalidVector(format!(
                    "vector field expects {dimensions} dimensions"
                )));
            }

            let mut out = Vec::with_capacity(4 + values.len() * 4);
            out.extend_from_slice(&(*dimensions as u32).to_be_bytes());
            for value in values {
                let value = value.as_f64().ok_or_else(|| {
                    CassieError::InvalidVector("vector field expects numeric values".into())
                })?;
                out.extend_from_slice(&(value as f32).to_be_bytes());
            }
            Ok((TYPE_VECTOR_F32, out))
        }
    }
}

fn decode_value(type_tag: u8, cursor: &mut Cursor<'_>) -> Result<serde_json::Value, CassieError> {
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
        TYPE_JSON => {
            let bytes = cursor.read_len_prefixed()?;
            serde_json::from_slice(&bytes)
                .map_err(|error| CassieError::Parse(format!("invalid JSON in row blob: {error}")))
        }
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
        TYPE_ARRAY => {
            let count = cursor.read_varint()? as usize;
            let mut values = Vec::with_capacity(count);
            for _ in 0..count {
                let value_type = cursor.read_u8()?;
                values.push(decode_value(value_type, cursor)?);
            }
            Ok(serde_json::Value::Array(values))
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
            for chunk in bytes[4..].chunks_exact(4) {
                let value = f32::from_be_bytes(chunk.try_into().map_err(|_| {
                    CassieError::Parse("invalid vector value in row blob".to_string())
                })?);
                let number = serde_json::Number::from_f64(value as f64).ok_or_else(|| {
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

fn write_varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
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

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn expect_bytes(&mut self, expected: &[u8]) -> Result<(), CassieError> {
        let actual = self.read_exact(expected.len())?;
        if actual != expected {
            return Err(CassieError::Parse("invalid row blob magic".into()));
        }
        Ok(())
    }

    fn read_u8(&mut self) -> Result<u8, CassieError> {
        let bytes = self.read_exact(1)?;
        Ok(bytes[0])
    }

    fn read_u32(&mut self) -> Result<u32, CassieError> {
        let bytes = self.read_exact(4)?;
        Ok(u32::from_be_bytes(bytes.try_into().map_err(|_| {
            CassieError::Parse("invalid u32 in row blob".to_string())
        })?))
    }

    fn read_i64(&mut self) -> Result<i64, CassieError> {
        let bytes = self.read_exact(8)?;
        Ok(i64::from_be_bytes(bytes.try_into().map_err(|_| {
            CassieError::Parse("invalid i64 in row blob".to_string())
        })?))
    }

    fn read_f64(&mut self) -> Result<f64, CassieError> {
        let bytes = self.read_exact(8)?;
        Ok(f64::from_be_bytes(bytes.try_into().map_err(|_| {
            CassieError::Parse("invalid f64 in row blob".to_string())
        })?))
    }

    fn read_varint(&mut self) -> Result<u64, CassieError> {
        let mut shift = 0;
        let mut value = 0u64;

        loop {
            if shift >= 64 {
                return Err(CassieError::Parse("row blob varint overflow".into()));
            }
            let byte = self.read_u8()?;
            value |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
        }
    }

    fn read_len_prefixed(&mut self) -> Result<Vec<u8>, CassieError> {
        let len = self.read_varint()? as usize;
        Ok(self.read_exact(len)?.to_vec())
    }

    fn skip_len_prefixed(&mut self) -> Result<(), CassieError> {
        let len = self.read_varint()? as usize;
        self.skip_exact(len)
    }

    fn skip_exact(&mut self, len: usize) -> Result<(), CassieError> {
        self.read_exact(len).map(|_| ())
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8], CassieError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| CassieError::Parse("row blob offset overflow".into()))?;
        if end > self.bytes.len() {
            return Err(CassieError::Parse("truncated row blob".into()));
        }

        let bytes = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(bytes)
    }
}

#[cfg(test)]
#[path = "row_blob/tests.rs"]
mod tests;
