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
    pub data_type: DataType,
    pub nullable: bool,
    pub retired: bool,
}

impl RowSchema {
    pub(crate) fn from_schema(schema: &Schema) -> Self {
        let fields = schema
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| RowFieldMeta {
                field_id: (index + 1) as u32,
                name: field.name.clone(),
                data_type: field.data_type.clone(),
                nullable: field.nullable,
                retired: false,
            })
            .collect::<Vec<_>>();

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

        self.fields.push(RowFieldMeta {
            field_id: self.next_field_id,
            name: field.name,
            data_type: field.data_type,
            nullable: field.nullable,
            retired: false,
        });
        self.next_field_id += 1;
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
        let mut fields = self
            .fields
            .iter()
            .filter(|field| !field.retired)
            .collect::<Vec<_>>();
        fields.sort_by_key(|field| field.field_id);
        fields
    }

    fn field_by_id(&self, field_id: u32) -> Option<&RowFieldMeta> {
        self.fields.iter().find(|field| field.field_id == field_id)
    }

    fn active_field_by_name(&self, name: &str) -> Option<&RowFieldMeta> {
        self.fields
            .iter()
            .find(|field| !field.retired && field.name.eq_ignore_ascii_case(name))
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
        let value = decode_value(type_tag, &mut cursor)?;

        let Some(field) = schema.field_by_id(field_id) else {
            continue;
        };
        if should_include_field(field, projection) {
            object.insert(field.name.clone(), value);
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

fn should_include_field(field: &RowFieldMeta, projection: Option<&HashSet<String>>) -> bool {
    if field.retired {
        return false;
    }

    projection.is_none_or(|projection| projection.contains(&field.name.to_ascii_lowercase()))
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
        DataType::Int => {
            let value = value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
                .ok_or_else(|| CassieError::InvalidVector("integer field expects i64".into()))?;
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
                    | TYPE_TIME | TYPE_TIMESTAMP | TYPE_ARRAY => {
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
mod tests {
    use super::*;

    #[test]
    fn should_decode_sparse_rows_without_field_names() {
        // Arrange
        let schema = RowSchema::from_schema(&Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
                    nullable: true,
                },
            ],
        });

        // Act
        let encoded = encode_row(&schema, &serde_json::json!({"score": 42})).unwrap();
        let decoded = decode_row(&schema, &encoded).unwrap();

        // Assert
        assert_eq!(decoded, serde_json::json!({"score": 42}));
        let raw = String::from_utf8_lossy(&encoded);
        assert!(!raw.contains("score"));
    }

    #[test]
    fn should_roundtrip_binary_temporal_uuid_array_fields() {
        // Arrange
        let schema = RowSchema::from_schema(&Schema {
            fields: vec![
                FieldSchema {
                    name: "id".to_string(),
                    data_type: DataType::Uuid,
                    nullable: true,
                },
                FieldSchema {
                    name: "created_on".to_string(),
                    data_type: DataType::Date,
                    nullable: true,
                },
                FieldSchema {
                    name: "created_at".to_string(),
                    data_type: DataType::Timestamp,
                    nullable: true,
                },
                FieldSchema {
                    name: "updated_at".to_string(),
                    data_type: DataType::Time,
                    nullable: true,
                },
                FieldSchema {
                    name: "ints".to_string(),
                    data_type: DataType::Array(Box::new(DataType::Int)),
                    nullable: true,
                },
            ],
        });
        let payload = serde_json::json!({
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "created_on": "2026-06-18",
            "created_at": "2026-06-18T12:34:56Z",
            "updated_at": "12:34:56",
            "ints": [1, 2, 3],
        });

        // Act
        let encoded = encode_row(&schema, &payload).unwrap();
        let mut cursor = Cursor::new(&encoded);
        cursor.expect_bytes(MAGIC).unwrap();
        let version = cursor.read_u8().unwrap();
        assert_eq!(version, FORMAT_VERSION);
        let _schema_version = cursor.read_u32().unwrap();
        let _flags = cursor.read_u8().unwrap();
        let field_count = cursor.read_varint().unwrap();
        for index in 0..field_count {
            let field_id = cursor.read_varint().unwrap();
            let tag = cursor.read_u8().unwrap();
            let value = decode_value(tag, &mut cursor);
            assert!(
                value.is_ok(),
                "field {index} id={field_id} tag={tag}: {value:?}"
            );
        }

        let decoded = decode_row(&schema, &encoded).unwrap();

        // Assert
        assert_eq!(
            decoded,
            serde_json::json!({
                "id": "550e8400-e29b-41d4-a716-446655440000",
                "created_on": "2026-06-18",
                "created_at": "2026-06-18T12:34:56Z",
                "updated_at": "12:34:56",
                "ints": [1, 2, 3],
            })
        );
        assert_eq!(&encoded[0..4], b"CRB1");
    }

    #[test]
    fn should_reject_invalid_uuid_values_during_row_blob_encoding() {
        // Arrange
        let schema = RowSchema::from_schema(&Schema {
            fields: vec![FieldSchema {
                name: "id".to_string(),
                data_type: DataType::Uuid,
                nullable: true,
            }],
        });
        let payload = serde_json::json!({"id": "not-a-uuid"});

        // Act
        let result = encode_row(&schema, &payload);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn should_retain_retired_field_ids() {
        // Arrange
        let mut schema = RowSchema::from_schema(&Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        });

        // Act
        assert!(schema.retire_field("title"));
        schema
            .add_field(FieldSchema {
                name: "status".to_string(),
                data_type: DataType::Text,
                nullable: true,
            })
            .unwrap();

        // Assert
        assert_eq!(schema.fields[0].field_id, 1);
        assert!(schema.fields[0].retired);
        assert_eq!(schema.fields[1].field_id, 2);
        assert!(!schema.fields[1].retired);
    }
}
