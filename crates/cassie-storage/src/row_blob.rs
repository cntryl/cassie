//! CRB1 row schema and byte framing used by the Midge adapter.

use std::collections::HashMap;

use cassie_core::types::{DataType, FieldSchema, Schema};
use cassie_sql::column_identifier::ColumnIdentifierPath;

use crate::StorageCodecError;

pub const MAGIC: &[u8; 4] = b"CRB1";
pub const FORMAT_VERSION: u8 = 1;
pub const TYPE_NULL: u8 = 0x00;
pub const TYPE_BOOL: u8 = 0x01;
pub const TYPE_I64: u8 = 0x02;
pub const TYPE_F64: u8 = 0x04;
pub const TYPE_STRING: u8 = 0x05;
pub const TYPE_UUID: u8 = 0x06;
pub const TYPE_JSON: u8 = 0x07;
pub const TYPE_VECTOR_F32: u8 = 0x08;
pub const TYPE_DATE: u8 = 0x09;
pub const TYPE_TIME: u8 = 0x0A;
pub const TYPE_TIMESTAMP: u8 = 0x0B;
pub const TYPE_ARRAY: u8 = 0x0C;
pub const TYPE_BYTEA: u8 = 0x0D;
pub const TYPE_EXTENDED: u8 = 0x0E;
pub const TYPE_COMPOSITE: u8 = 0x0F;
pub const TYPE_DOMAIN: u8 = 0x10;
pub const TYPE_ENUM: u8 = 0x11;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RowSchema {
    #[serde(default)]
    pub relation_id: u64,
    pub schema_version: u32,
    pub next_field_id: u32,
    pub fields: Vec<RowFieldMeta>,
    #[serde(default)]
    pub column_revision_floor: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RowFieldMeta {
    pub field_id: u32,
    pub name: String,
    #[serde(default)]
    pub normalized_name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub data_type: DataType,
    pub nullable: bool,
    pub retired: bool,
    #[serde(default = "initial_added_version")]
    pub added_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowSchemaError {
    Unsupported(String),
}

impl std::fmt::Display for RowSchemaError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for RowSchemaError {}

impl From<RowSchemaError> for StorageCodecError {
    fn from(error: RowSchemaError) -> Self {
        match error {
            RowSchemaError::Unsupported(message) => Self::UnsupportedValue(message),
        }
    }
}

fn initial_added_version() -> u32 {
    1
}

impl RowSchema {
    #[must_use]
    pub fn from_schema(schema: &Schema) -> Self {
        let mut fields = schema
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| RowFieldMeta {
                field_id: field_id_from_index(index),
                name: field.name.clone(),
                normalized_name: ColumnIdentifierPath::stored_field_key(&field.name),
                aliases: Vec::new(),
                data_type: field.data_type.clone(),
                nullable: field.nullable,
                retired: false,
                added_version: 1,
            })
            .collect::<Vec<_>>();
        hydrate_normalized_names(fields.as_mut_slice());

        Self {
            relation_id: 0,
            schema_version: 1,
            next_field_id: next_field_id_for_len(fields.len()),
            fields,
            column_revision_floor: 0,
        }
    }

    #[must_use]
    pub fn active_schema(&self) -> Schema {
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

    /// # Errors
    /// Returns a schema error when the requested field conflicts with or cannot be resolved in the schema.
    pub fn add_field(&mut self, field: FieldSchema) -> Result<(), RowSchemaError> {
        if self
            .fields
            .iter()
            .any(|entry| entry.name == field.name && !entry.retired)
        {
            return Err(RowSchemaError::Unsupported(format!(
                "field '{0}' already exists",
                field.name
            )));
        }

        let normalized_name = ColumnIdentifierPath::stored_field_key(&field.name);
        self.fields.push(RowFieldMeta {
            field_id: self.next_field_id,
            name: field.name,
            normalized_name,
            aliases: Vec::new(),
            data_type: field.data_type,
            nullable: field.nullable,
            retired: false,
            added_version: self.schema_version + 1,
        });
        self.next_field_id += 1;
        self.schema_version += 1;
        Ok(())
    }

    /// # Errors
    /// Returns a schema error when the requested field conflicts with or cannot be resolved in the schema.
    pub fn rename_field(&mut self, current: &str, next: &str) -> Result<(), RowSchemaError> {
        if self
            .fields
            .iter()
            .any(|entry| entry.name == next && !entry.retired)
        {
            return Err(RowSchemaError::Unsupported(format!(
                "field '{next}' already exists"
            )));
        }

        let Some(field_index) = self
            .fields
            .iter()
            .position(|entry| entry.name == current && !entry.retired)
        else {
            return Err(RowSchemaError::Unsupported(format!(
                "field '{current}' not found"
            )));
        };

        let next_normalized = ColumnIdentifierPath::stored_field_key(next);
        for (index, entry) in self.fields.iter_mut().enumerate() {
            if index != field_index {
                entry.aliases.retain(|alias| {
                    ColumnIdentifierPath::stored_field_key(alias) != next_normalized
                });
            }
        }
        let field = &mut self.fields[field_index];
        push_field_alias(field, field.name.clone());
        field.name = next.to_string();
        field.normalized_name = next_normalized;
        self.schema_version += 1;
        Ok(())
    }

    pub fn retire_field(&mut self, name: &str) -> bool {
        let Some(field) = self
            .fields
            .iter_mut()
            .find(|entry| entry.name == name && !entry.retired)
        else {
            return false;
        };

        push_field_alias(field, field.name.clone());
        field.retired = true;
        self.schema_version += 1;
        true
    }

    #[must_use]
    pub fn active_fields_by_id(&self) -> Vec<&RowFieldMeta> {
        self.fields.iter().filter(|field| !field.retired).collect()
    }
}

fn hydrate_normalized_names(fields: &mut [RowFieldMeta]) {
    for field in fields {
        if field.normalized_name.is_empty() {
            field.normalized_name = ColumnIdentifierPath::stored_field_key(&field.name);
        }
    }
}

fn field_id_from_index(index: usize) -> u32 {
    u32::try_from(index + 1).expect("row schema field ids fit in u32")
}

fn next_field_id_for_len(len: usize) -> u32 {
    u32::try_from(len)
        .expect("row schema field ids fit in u32")
        .checked_add(1)
        .expect("next row schema field id fits in u32")
}

fn push_field_alias(field: &mut RowFieldMeta, alias: String) {
    if field.aliases.iter().any(|candidate| {
        ColumnIdentifierPath::stored_field_key(candidate)
            == ColumnIdentifierPath::stored_field_key(&alias)
    }) {
        return;
    }
    field.aliases.push(alias);
}

#[derive(Debug)]
pub enum RowBlobEncodeError<E> {
    Layout(StorageCodecError),
    FieldValue(E),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RowBlobValueError {
    #[error("{0}")]
    InvalidValue(String),
    #[error("{0}")]
    InvalidEncoding(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("{message}")]
    TypeInput {
        sqlstate: &'static str,
        message: String,
    },
}

mod encode;

/// # Errors
/// Returns an encoding error when a document value cannot be represented by the schema or value encoder.
pub fn encode_row(
    schema: &RowSchema,
    document: &serde_json::Value,
) -> Result<Vec<u8>, RowBlobEncodeError<RowBlobValueError>> {
    encode_row_with(schema, document, encode::encode_value)
}

/// # Errors
/// Returns an encoding error when a document value cannot be represented by the schema or value encoder.
pub fn encode_row_with<E>(
    schema: &RowSchema,
    document: &serde_json::Value,
    mut encode_value: impl FnMut(&DataType, &serde_json::Value) -> Result<(u8, Vec<u8>), E>,
) -> Result<Vec<u8>, RowBlobEncodeError<E>> {
    let object = document.as_object().ok_or_else(|| {
        RowBlobEncodeError::Layout(StorageCodecError::InvalidValue(
            "document must be object".to_owned(),
        ))
    })?;

    let mut fields = Vec::new();
    for field in schema.active_fields_by_id() {
        let Some(value) = object.get(&field.name) else {
            continue;
        };
        let (type_tag, encoded) =
            encode_value(&field.data_type, value).map_err(RowBlobEncodeError::FieldValue)?;
        fields.push((field.field_id, type_tag, encoded));
    }

    let max_field_id = schema
        .fields
        .iter()
        .map(|field| field.field_id)
        .max()
        .unwrap_or_default();
    let bitmap_len = usize::try_from(max_field_id.div_ceil(8)).unwrap_or(usize::MAX);
    let mut presence = vec![0_u8; bitmap_len];
    let mut nulls = vec![0_u8; bitmap_len];
    let mut payload = Vec::new();
    let mut directory = Vec::with_capacity(fields.len());
    for (field_id, type_tag, encoded) in &fields {
        set_field_bit(&mut presence, *field_id);
        if *type_tag == TYPE_NULL {
            set_field_bit(&mut nulls, *field_id);
        }
        let offset = u32::try_from(payload.len()).map_err(|_| {
            RowBlobEncodeError::Layout(StorageCodecError::InvalidEncoding(
                "row blob payload offset overflow".to_owned(),
            ))
        })?;
        let len = u32::try_from(encoded.len()).map_err(|_| {
            RowBlobEncodeError::Layout(StorageCodecError::InvalidEncoding(
                "row blob field length overflow".to_owned(),
            ))
        })?;
        payload.extend_from_slice(encoded);
        directory.push((*field_id, *type_tag, offset, len));
    }

    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.push(FORMAT_VERSION);
    out.extend_from_slice(&schema.schema_version.to_be_bytes());
    out.extend_from_slice(&max_field_id.to_be_bytes());
    out.extend_from_slice(
        &u32::try_from(bitmap_len)
            .map_err(|_| {
                RowBlobEncodeError::Layout(StorageCodecError::InvalidEncoding(
                    "row blob bitmap length overflow".to_owned(),
                ))
            })?
            .to_be_bytes(),
    );
    out.extend_from_slice(&presence);
    out.extend_from_slice(&nulls);
    out.extend_from_slice(
        &u32::try_from(directory.len())
            .map_err(|_| {
                RowBlobEncodeError::Layout(StorageCodecError::InvalidEncoding(
                    "row blob field count overflow".to_owned(),
                ))
            })?
            .to_be_bytes(),
    );
    for (field_id, type_tag, offset, len) in directory {
        out.extend_from_slice(&field_id.to_be_bytes());
        out.push(type_tag);
        out.extend_from_slice(&offset.to_be_bytes());
        out.extend_from_slice(&len.to_be_bytes());
    }
    out.extend_from_slice(&payload);

    Ok(out)
}

fn set_field_bit(bitmap: &mut [u8], field_id: u32) {
    let Some(zero_based) = field_id.checked_sub(1) else {
        return;
    };
    let byte = usize::try_from(zero_based / 8).unwrap_or(usize::MAX);
    if let Some(slot) = bitmap.get_mut(byte) {
        *slot |= 1 << (zero_based % 8);
    }
}

#[derive(Debug, Clone, Copy)]
pub enum RowBlobField<'a> {
    SqlNull,
    Encoded { type_tag: u8, bytes: &'a [u8] },
}

pub struct RowBlobRecord<'a> {
    presence: &'a [u8],
    nulls: &'a [u8],
    entries: HashMap<u32, (u8, usize, usize)>,
    payload: &'a [u8],
}

impl<'a> RowBlobRecord<'a> {
    /// # Errors
    /// Returns a codec error when row framing, field identifiers, or payload bytes are invalid.
    pub fn parse(row: &'a [u8]) -> Result<Self, StorageCodecError> {
        let mut cursor = Cursor::new(row);
        cursor.expect_bytes(MAGIC)?;
        let version = cursor.read_u8()?;
        if version != FORMAT_VERSION {
            return Err(invalid(format!(
                "unsupported row blob format version {version}"
            )));
        }
        let _schema_version = cursor.read_u32()?;
        let _max_field_id = cursor.read_u32()?;
        let bitmap_len = usize::try_from(cursor.read_u32()?)
            .map_err(|_| invalid("row blob bitmap length overflow"))?;
        let presence = cursor.read_exact(bitmap_len)?;
        let nulls = cursor.read_exact(bitmap_len)?;
        let field_count = cursor.read_u32()?;
        let mut entries = HashMap::new();
        for _ in 0..field_count {
            let field_id = cursor.read_u32()?;
            let type_tag = cursor.read_u8()?;
            let offset = usize::try_from(cursor.read_u32()?)
                .map_err(|_| invalid("row blob field offset overflow"))?;
            let len = usize::try_from(cursor.read_u32()?)
                .map_err(|_| invalid("row blob field length overflow"))?;
            entries.insert(field_id, (type_tag, offset, len));
        }
        let payload = cursor.remaining();
        let mut ranges = Vec::with_capacity(entries.len());
        for (_, offset, len) in entries.values() {
            let Some(end) = offset.checked_add(*len) else {
                return Err(invalid("invalid row blob field bounds"));
            };
            if end > payload.len() {
                return Err(invalid("invalid row blob field bounds"));
            }
            ranges.push((*offset, end));
        }
        ranges.sort_unstable();
        let mut covered = 0usize;
        for (start, end) in ranges {
            if start != covered {
                return Err(invalid("row blob field ranges must partition the payload"));
            }
            covered = end;
        }
        if covered != payload.len() {
            return Err(invalid("row blob contains trailing bytes"));
        }
        Ok(Self {
            presence,
            nulls,
            entries,
            payload,
        })
    }

    /// # Errors
    /// Returns a codec error when row framing, field identifiers, or payload bytes are invalid.
    pub fn field(&self, field_id: u32) -> Result<Option<RowBlobField<'a>>, StorageCodecError> {
        if !field_bit(self.presence, field_id) {
            return Ok(None);
        }
        if field_bit(self.nulls, field_id) {
            return Ok(Some(RowBlobField::SqlNull));
        }
        let (type_tag, offset, len) = self
            .entries
            .get(&field_id)
            .copied()
            .ok_or_else(|| invalid("row blob field missing directory entry"))?;
        let end = offset
            .checked_add(len)
            .ok_or_else(|| invalid("invalid row blob field bounds"))?;
        let bytes = self
            .payload
            .get(offset..end)
            .ok_or_else(|| invalid("invalid row blob field bounds"))?;
        Ok(Some(RowBlobField::Encoded { type_tag, bytes }))
    }
}

#[must_use]
pub fn field_bit(bitmap: &[u8], field_id: u32) -> bool {
    field_id.checked_sub(1).is_some_and(|zero_based| {
        let byte = usize::try_from(zero_based / 8).unwrap_or(usize::MAX);
        bitmap
            .get(byte)
            .is_some_and(|value| value & (1 << (zero_based % 8)) != 0)
    })
}

pub struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    /// # Errors
    /// Returns a codec error when row framing, field identifiers, or payload bytes are invalid.
    pub fn expect_bytes(&mut self, expected: &[u8]) -> Result<(), StorageCodecError> {
        if self.read_exact(expected.len())? == expected {
            Ok(())
        } else {
            Err(invalid("invalid row blob magic"))
        }
    }

    /// # Errors
    /// Returns a codec error when row framing, field identifiers, or payload bytes are invalid.
    pub fn read_u8(&mut self) -> Result<u8, StorageCodecError> {
        Ok(self.read_exact(1)?[0])
    }

    /// # Errors
    /// Returns a codec error when row framing, field identifiers, or payload bytes are invalid.
    pub fn read_u32(&mut self) -> Result<u32, StorageCodecError> {
        let bytes = self.read_exact(4)?;
        Ok(u32::from_be_bytes(
            bytes
                .try_into()
                .map_err(|_| invalid("invalid u32 in row blob"))?,
        ))
    }

    /// # Errors
    /// Returns a codec error when row framing, field identifiers, or payload bytes are invalid.
    pub fn read_i64(&mut self) -> Result<i64, StorageCodecError> {
        let bytes = self.read_exact(8)?;
        Ok(i64::from_be_bytes(
            bytes
                .try_into()
                .map_err(|_| invalid("invalid i64 in row blob"))?,
        ))
    }

    /// # Errors
    /// Returns a codec error when row framing, field identifiers, or payload bytes are invalid.
    pub fn read_f64(&mut self) -> Result<f64, StorageCodecError> {
        let bytes = self.read_exact(8)?;
        Ok(f64::from_be_bytes(
            bytes
                .try_into()
                .map_err(|_| invalid("invalid f64 in row blob"))?,
        ))
    }

    /// # Errors
    /// Returns a codec error when row framing, field identifiers, or payload bytes are invalid.
    pub fn read_varint(&mut self) -> Result<u64, StorageCodecError> {
        let mut shift = 0;
        let mut value = 0_u64;
        loop {
            if shift >= 64 {
                return Err(invalid("row blob varint overflow"));
            }
            let byte = self.read_u8()?;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
        }
    }

    /// # Errors
    /// Returns a codec error when row framing, field identifiers, or payload bytes are invalid.
    pub fn read_len_prefixed(&mut self) -> Result<Vec<u8>, StorageCodecError> {
        let len = usize::try_from(self.read_varint()?)
            .map_err(|_| invalid("row blob length overflow"))?;
        Ok(self.read_exact(len)?.to_vec())
    }

    /// # Errors
    /// Returns a codec error when row framing, field identifiers, or payload bytes are invalid.
    pub fn read_exact(&mut self, len: usize) -> Result<&'a [u8], StorageCodecError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| invalid("row blob offset overflow"))?;
        if end > self.bytes.len() {
            return Err(invalid("truncated row blob"));
        }
        let bytes = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(bytes)
    }

    #[must_use]
    pub fn remaining(&self) -> &'a [u8] {
        &self.bytes[self.offset..]
    }
}

fn invalid(message: impl Into<String>) -> StorageCodecError {
    StorageCodecError::InvalidEncoding(message.into())
}

mod decode;
pub use decode::{
    decode_compact_field_value, decode_compact_scalar, decode_projected_row,
    decode_projected_row_matching, decode_projected_row_with_aliases, decode_row,
    encode_compact_sql_null, split_vector_row, CompactScalar, PackedVector, RowBlobDecodeError,
    VectorColumn, VectorRowParts,
};
pub use encode::{canonical_bytea_text, encode_compact_value};

#[cfg(test)]
#[path = "row_blob_tests.rs"]
mod tests;
