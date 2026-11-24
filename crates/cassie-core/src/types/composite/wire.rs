//! Storage JSON and PostgreSQL binary framing for record values.
use serde_json::{Map, Number};

use super::{CompositeRef, RecordValue, RECORD_STORAGE_KEY};
use crate::types::{DataType, Value};

/// A stored record document that cannot be read as a value of its column type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRecordError(String);

impl StoredRecordError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for StoredRecordError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for StoredRecordError {}

/// The JSON document that carries a record through the row store. Storage
/// re-encodes it as a CRB1 composite payload; SQL NULL is plain JSON `null`.
///
/// `fields` follows the physical layout of the descriptor: one entry per slot,
/// with `null` at the position of every dropped attribute.
#[must_use]
pub fn record_to_storage_json(record: &RecordValue) -> serde_json::Value {
    let descriptor = record.descriptor();
    let mut live = record
        .fields()
        .iter()
        .zip(&descriptor.fields)
        .map(|(value, field)| field_to_storage_json(value, &field.data_type));
    let slots = (0..descriptor.physical_len())
        .map(|slot| {
            if descriptor.dropped_positions.contains(&slot) {
                serde_json::Value::Null
            } else {
                live.next().unwrap_or(serde_json::Value::Null)
            }
        })
        .collect();
    let mut body = Map::new();
    body.insert("oid".to_string(), serde_json::Value::from(descriptor.oid));
    body.insert(
        "revision".to_string(),
        serde_json::Value::from(descriptor.revision),
    );
    body.insert("fields".to_string(), serde_json::Value::Array(slots));
    let mut object = Map::new();
    object.insert(
        RECORD_STORAGE_KEY.to_string(),
        serde_json::Value::Object(body),
    );
    serde_json::Value::Object(object)
}

/// A `json`/`jsonb` field is stored as its document, as a column of that type
/// is, even when the evaluator still carries it as text.
fn field_to_storage_json(value: &Value, data_type: &DataType) -> serde_json::Value {
    match value {
        Value::String(text) if data_type.flattened().is_json() => {
            serde_json::from_str(text).unwrap_or_else(|_| serde_json::Value::String(text.clone()))
        }
        Value::Null => serde_json::Value::Null,
        Value::Bool(flag) => serde_json::Value::Bool(*flag),
        Value::Int64(number) => serde_json::Value::Number((*number).into()),
        Value::Float64(number) => Number::from_f64(*number).map_or_else(
            || serde_json::Value::String(crate::types::float_text::format(*number)),
            serde_json::Value::Number,
        ),
        Value::String(text) => serde_json::Value::String(text.clone()),
        Value::Vector(vector) => serde_json::Value::Array(
            vector
                .values
                .iter()
                .map(|component| {
                    Number::from_f64(f64::from(*component))
                        .map_or(serde_json::Value::Null, serde_json::Value::Number)
                })
                .collect(),
        ),
        Value::Array(array) => array.to_storage_json(),
        Value::Json(document) => document.clone(),
        Value::Typed(typed) => typed.to_json(),
    }
}

/// Rebuilds a record of `descriptor` from its storage JSON, converting each
/// live field with `field_from_json`.
///
/// A value written under an older revision carries fewer physical slots; the
/// attributes added since read as NULL and dropped slots are skipped.
///
/// # Errors
///
/// Returns [`StoredRecordError`] when the document is not a record of this
/// type, carries more slots than the descriptor, or a field cannot be read.
pub fn record_from_storage_json(
    json: &serde_json::Value,
    descriptor: &CompositeRef,
    field_from_json: &mut dyn FnMut(
        &serde_json::Value,
        &DataType,
    ) -> Result<Value, StoredRecordError>,
) -> Result<RecordValue, StoredRecordError> {
    let mismatch = || {
        StoredRecordError::new(format!(
            "stored value does not match composite type {}",
            descriptor.name
        ))
    };
    let body = json
        .as_object()
        .and_then(|object| object.get(RECORD_STORAGE_KEY))
        .and_then(serde_json::Value::as_object)
        .ok_or_else(mismatch)?;
    if body.get("oid").and_then(serde_json::Value::as_u64) != Some(u64::from(descriptor.oid)) {
        return Err(mismatch());
    }
    let stored = body
        .get("fields")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(mismatch)?;
    if stored.len() > descriptor.physical_len() {
        return Err(StoredRecordError::new(format!(
            "stored value of composite type {} has {} fields but the type has {}",
            descriptor.name,
            stored.len(),
            descriptor.physical_len()
        )));
    }
    let mut values = Vec::with_capacity(descriptor.fields.len());
    for (field, slot) in descriptor
        .fields
        .iter()
        .zip(descriptor.physical_positions())
    {
        values.push(match stored.get(slot) {
            None | Some(serde_json::Value::Null) => Value::Null,
            Some(value) => field_from_json(value, field.data_type.flattened())?,
        });
    }
    RecordValue::new(descriptor.clone(), values).map_err(StoredRecordError::new)
}

/// The type OID `record_send` and `record_recv` carry for a field: a domain
/// field carries the domain's own OID, as PostgreSQL does.
fn field_oid(data_type: &DataType) -> u32 {
    match data_type {
        DataType::Domain(domain) => domain.oid,
        other => u32::try_from(other.type_oid()).unwrap_or(0),
    }
}

/// `record_send`: field count, then per field its type OID and length-prefixed
/// bytes (`-1` for NULL), all big-endian.
///
/// # Errors
///
/// Returns the first field encoding error, or a message when a length does not fit.
pub fn send_record_binary<E: From<String>>(
    record: &RecordValue,
    mut field_binary: impl FnMut(&Value, &DataType) -> Result<Vec<u8>, E>,
) -> Result<Vec<u8>, E> {
    let count = i32::try_from(record.fields().len())
        .map_err(|_| E::from("record has too many fields".to_string()))?;
    let mut out = count.to_be_bytes().to_vec();
    for (value, field) in record.fields().iter().zip(&record.descriptor().fields) {
        let oid = field_oid(&field.data_type);
        out.extend_from_slice(&oid.to_be_bytes());
        if value.is_null() {
            out.extend_from_slice(&(-1_i32).to_be_bytes());
            continue;
        }
        let bytes = field_binary(value, field.data_type.flattened())?;
        let length = i32::try_from(bytes.len())
            .map_err(|_| E::from("record field is too large".to_string()))?;
        out.extend_from_slice(&length.to_be_bytes());
        out.extend_from_slice(&bytes);
    }
    Ok(out)
}

/// `record_recv`: validates the frame against `descriptor` and converts each
/// non-NULL field with `field_value`.
///
/// # Errors
///
/// Returns a message for a truncated frame, a wrong field count or a field
/// whose declared type OID differs from the descriptor.
pub fn receive_record_binary(
    bytes: &[u8],
    descriptor: &CompositeRef,
    mut field_value: impl FnMut(&DataType, &[u8]) -> Result<Value, String>,
) -> Result<RecordValue, String> {
    let mut cursor = bytes;
    let count = read_i32(&mut cursor)?;
    if usize::try_from(count).ok() != Some(descriptor.fields.len()) {
        return Err("wrong number of columns in record".to_string());
    }
    let mut values = Vec::with_capacity(descriptor.fields.len());
    for field in &descriptor.fields {
        let oid = read_i32(&mut cursor)?;
        let expected = i32::try_from(field_oid(&field.data_type)).unwrap_or(0);
        if oid != expected {
            return Err(format!(
                "wrong data type in record column \"{}\": expected {expected}, received {oid}",
                field.name
            ));
        }
        let length = read_i32(&mut cursor)?;
        if length == -1 {
            values.push(Value::Null);
            continue;
        }
        let length = usize::try_from(length)
            .map_err(|_| "insufficient data left in record message".to_string())?;
        if cursor.len() < length {
            return Err("insufficient data left in record message".to_string());
        }
        let (field_bytes, rest) = cursor.split_at(length);
        cursor = rest;
        values.push(field_value(field.data_type.flattened(), field_bytes)?);
    }
    if !cursor.is_empty() {
        return Err("improper binary format in record column".to_string());
    }
    RecordValue::new(descriptor.clone(), values)
}

fn read_i32(cursor: &mut &[u8]) -> Result<i32, String> {
    let Some((head, rest)) = cursor.split_first_chunk::<4>() else {
        return Err("insufficient data left in record message".to_string());
    };
    *cursor = rest;
    Ok(i32::from_be_bytes(*head))
}

/// A best-effort `typsend` for one field, used where no wire layer supplies
/// the exact PostgreSQL binary codec for the field's type.
///
/// # Errors
///
/// Returns a message when an integer does not fit its declared width.
pub fn default_field_binary(value: &Value, data_type: &DataType) -> Result<Vec<u8>, String> {
    let out_of_range = || "record integer field is out of range".to_string();
    Ok(match (value, data_type) {
        (Value::Bool(flag), _) => vec![u8::from(*flag)],
        (Value::Int64(number), DataType::SmallInt) => i16::try_from(*number)
            .map_err(|_| out_of_range())?
            .to_be_bytes()
            .to_vec(),
        (Value::Int64(number), DataType::Int) => i32::try_from(*number)
            .map_err(|_| out_of_range())?
            .to_be_bytes()
            .to_vec(),
        (Value::Int64(number), DataType::ObjectId(_)) => u32::try_from(*number)
            .map_err(|_| out_of_range())?
            .to_be_bytes()
            .to_vec(),
        (Value::Int64(number), _) => number.to_be_bytes().to_vec(),
        (Value::Float64(number), _) => number.to_be_bytes().to_vec(),
        (Value::Typed(typed), _) => typed.send_binary(),
        (Value::String(text), _) => text.as_bytes().to_vec(),
        (Value::Json(document), _) => document.to_string().into_bytes(),
        (Value::Array(array), _) => array.to_nested_json().to_string().into_bytes(),
        (Value::Vector(vector), _) => vector
            .values
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
            .into_bytes(),
        (Value::Null, _) => Vec::new(),
    })
}
