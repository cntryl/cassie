use std::{collections::HashSet, hash::BuildHasher};

use cassie_core::types::{extended::ExtendedType, ArrayDimension, DataType, SqlArray, Value};
use cassie_sql::column_identifier::ColumnIdentifierPath;

use super::{
    invalid, Cursor, RowBlobField, RowBlobRecord, RowFieldMeta, RowSchema, TYPE_ARRAY, TYPE_BOOL,
    TYPE_BYTEA, TYPE_COMPOSITE, TYPE_DATE, TYPE_DOMAIN, TYPE_ENUM, TYPE_EXTENDED, TYPE_F64,
    TYPE_I64, TYPE_JSON, TYPE_NULL, TYPE_STRING, TYPE_TIME, TYPE_TIMESTAMP, TYPE_UUID,
    TYPE_VECTOR_F32,
};
use crate::StorageCodecError;

#[derive(Debug)]
pub enum RowBlobDecodeError<E> {
    Codec(StorageCodecError),
    Json(E),
    DataCorruption {
        sqlstate: &'static str,
        message: String,
    },
}

impl<E> From<StorageCodecError> for RowBlobDecodeError<E> {
    fn from(error: StorageCodecError) -> Self {
        Self::Codec(error)
    }
}

/// # Errors
/// Returns a codec error for malformed or schema-inconsistent row data, or forwards an error from the JSON decoder.
pub fn decode_row<E>(
    schema: &RowSchema,
    row: &[u8],
    decode_json: impl Fn(&[u8]) -> Result<serde_json::Value, E>,
) -> Result<serde_json::Value, RowBlobDecodeError<E>> {
    decode_with_projection::<E, std::collections::hash_map::RandomState>(
        schema,
        row,
        None,
        false,
        &decode_json,
    )
}

/// # Errors
/// Returns a codec error for malformed or schema-inconsistent row data, or forwards an error from the JSON decoder.
pub fn decode_projected_row<E>(
    schema: &RowSchema,
    row: &[u8],
    projection: &HashSet<String, impl BuildHasher>,
    decode_json: impl Fn(&[u8]) -> Result<serde_json::Value, E>,
) -> Result<serde_json::Value, RowBlobDecodeError<E>> {
    decode_with_projection(schema, row, Some(projection), false, &decode_json)
}

/// # Errors
/// Returns a codec error for malformed or schema-inconsistent row data, or forwards an error from the JSON decoder.
pub fn decode_projected_row_with_aliases<E>(
    schema: &RowSchema,
    row: &[u8],
    projection: &HashSet<String, impl BuildHasher>,
    decode_json: impl Fn(&[u8]) -> Result<serde_json::Value, E>,
) -> Result<serde_json::Value, RowBlobDecodeError<E>> {
    decode_with_projection(schema, row, Some(projection), true, &decode_json)
}

/// # Errors
/// Returns a codec error for malformed or schema-inconsistent row data, or forwards an error from the JSON decoder.
pub fn decode_projected_row_matching<E>(
    schema: &RowSchema,
    row: &[u8],
    projection: &HashSet<String, impl BuildHasher>,
    filter_field: &str,
    filter_value: &serde_json::Value,
    include_historical_aliases: bool,
    decode_json: impl Fn(&[u8]) -> Result<serde_json::Value, E>,
) -> Result<Option<serde_json::Value>, RowBlobDecodeError<E>> {
    let record = RowBlobRecord::parse(row)?;
    let requested = ColumnIdentifierPath::parse(filter_field).map_or_else(
        |_| filter_field.to_ascii_lowercase(),
        |path| path.field_lookup_key(),
    );
    let active = schema
        .fields
        .iter()
        .filter(|field| !field.retired)
        .map(|field| ColumnIdentifierPath::from_field_name(&field.name).lookup_key())
        .collect::<HashSet<_>>();
    let mut object = serde_json::Map::new();
    let mut saw_filter = false;
    let mut matched_filter = false;
    for field in &schema.fields {
        let names = names_for_field(field, Some(projection), include_historical_aliases, &active);
        let filter_match =
            field_matches_name(field, &requested, include_historical_aliases, &active);
        if names.is_empty() && !filter_match {
            continue;
        }
        let Some(value) = decode_field(&record, field, &decode_json)? else {
            continue;
        };
        if filter_match {
            saw_filter = true;
            matched_filter = value == *filter_value;
        }
        for name in names {
            object.insert(name, value.clone());
        }
    }
    Ok((saw_filter && matched_filter).then_some(object.into()))
}

/// # Errors
/// Returns a codec error for malformed or schema-inconsistent row data, or forwards an error from the JSON decoder.
pub fn decode_compact_field_value<E>(
    data_type: &DataType,
    bytes: &[u8],
    decode_json: impl Fn(&[u8]) -> Result<serde_json::Value, E>,
) -> Result<Option<serde_json::Value>, RowBlobDecodeError<E>> {
    let (&tag, payload) = bytes
        .split_first()
        .ok_or_else(|| invalid("empty compact value"))?;
    if tag == TYPE_NULL {
        if !payload.is_empty() {
            return Err(invalid("row blob field payload has unconsumed bytes").into());
        }
        return Ok((!matches!(data_type, DataType::Json | DataType::Jsonb))
            .then_some(serde_json::Value::Null));
    }
    let expected = expected_type_tag(data_type);
    if tag != expected {
        return Err(invalid(format!(
            "stored type tag {tag} but the declared type uses type tag {expected}"
        ))
        .into());
    }
    if let DataType::Extended(kind) = data_type {
        let oid = payload
            .first_chunk::<4>()
            .ok_or_else(|| invalid("truncated compact extended type OID"))?;
        let stored_oid = u32::from_be_bytes(*oid);
        if stored_oid != kind.type_oid() {
            return Err(invalid(format!(
                "stored type OID {stored_oid} but the declared type uses OID {}",
                kind.type_oid()
            ))
            .into());
        }
    }
    let value = decode_payload(tag, payload, &decode_json)?;
    check_column(data_type, &value)?;
    check_integer_width(data_type, &value)?;
    Ok(Some(value))
}

fn decode_with_projection<E, S: BuildHasher>(
    schema: &RowSchema,
    row: &[u8],
    projection: Option<&HashSet<String, S>>,
    aliases: bool,
    decode_json: &impl Fn(&[u8]) -> Result<serde_json::Value, E>,
) -> Result<serde_json::Value, RowBlobDecodeError<E>> {
    let record = RowBlobRecord::parse(row)?;
    let active = schema
        .fields
        .iter()
        .filter(|field| !field.retired)
        .map(|field| ColumnIdentifierPath::from_field_name(&field.name).lookup_key())
        .collect::<HashSet<_>>();
    let mut object = serde_json::Map::new();
    for field in &schema.fields {
        let names = names_for_field(field, projection, aliases, &active);
        if names.is_empty() {
            continue;
        }
        if let Some(value) = decode_field(&record, field, decode_json)? {
            for name in names {
                object.insert(name, value.clone());
            }
        }
    }
    Ok(serde_json::Value::Object(object))
}

fn decode_field<E>(
    record: &RowBlobRecord<'_>,
    field: &RowFieldMeta,
    decode_json: &impl Fn(&[u8]) -> Result<serde_json::Value, E>,
) -> Result<Option<serde_json::Value>, RowBlobDecodeError<E>> {
    let Some(stored) = record.field(field.field_id)? else {
        return Ok(None);
    };
    let (tag, bytes) = match stored {
        RowBlobField::SqlNull => {
            return Ok(
                (!matches!(field.data_type, DataType::Json | DataType::Jsonb))
                    .then_some(serde_json::Value::Null),
            );
        }
        RowBlobField::Encoded { type_tag, bytes } => (type_tag, bytes),
    };
    let expected = expected_type_tag(&field.data_type);
    if tag != expected {
        return Err(invalid(format!(
            "row blob field '{}' stores type tag {tag} but its schema declares type tag {expected}",
            field.name
        ))
        .into());
    }
    let value = decode_payload(tag, bytes, decode_json)?;
    check_column(&field.data_type, &value)?;
    Ok(Some(value))
}

fn names_for_field<S: BuildHasher>(
    field: &RowFieldMeta,
    projection: Option<&HashSet<String, S>>,
    aliases: bool,
    active: &HashSet<String>,
) -> Vec<String> {
    let Some(projection) = projection else {
        return (!field.retired)
            .then(|| field.name.clone())
            .into_iter()
            .collect();
    };
    let mut names = Vec::new();
    let key = ColumnIdentifierPath::from_field_name(&field.name).lookup_key();
    if !field.retired && projection.contains(&key) {
        names.push(field.name.clone());
    }
    if aliases {
        for alias in &field.aliases {
            let key = ColumnIdentifierPath::from_field_name(alias).lookup_key();
            if projection.contains(&key) && !active.contains(&key) && !names.contains(alias) {
                names.push(alias.clone());
            }
        }
    }
    names
}

fn field_matches_name(
    field: &RowFieldMeta,
    requested: &str,
    include_aliases: bool,
    active: &HashSet<String>,
) -> bool {
    (!field.retired && ColumnIdentifierPath::from_field_name(&field.name).lookup_key() == requested)
        || (include_aliases
            && field.aliases.iter().any(|alias| {
                ColumnIdentifierPath::from_field_name(alias).lookup_key() == requested
            })
            && !active.contains(requested))
}

fn expected_type_tag(data_type: &DataType) -> u8 {
    match data_type {
        DataType::Null => TYPE_NULL,
        DataType::SmallInt | DataType::Int | DataType::BigInt | DataType::ObjectId(_) => TYPE_I64,
        DataType::Float => TYPE_F64,
        DataType::Boolean => TYPE_BOOL,
        DataType::Name
        | DataType::Text
        | DataType::Char { .. }
        | DataType::Varchar { .. }
        | DataType::JsonPath => TYPE_STRING,
        DataType::Uuid => TYPE_UUID,
        DataType::Date => TYPE_DATE,
        DataType::Time => TYPE_TIME,
        DataType::Timestamp => TYPE_TIMESTAMP,
        DataType::Bytea => TYPE_BYTEA,
        DataType::Array(_) => TYPE_ARRAY,
        DataType::Json | DataType::Jsonb => TYPE_JSON,
        DataType::Vector(_) => TYPE_VECTOR_F32,
        DataType::Extended(_) => TYPE_EXTENDED,
        DataType::Composite(_) => TYPE_COMPOSITE,
        DataType::Domain(_) => TYPE_DOMAIN,
        DataType::Enum(_) => TYPE_ENUM,
    }
}

fn decode_payload<E>(
    tag: u8,
    bytes: &[u8],
    decode_json: &impl Fn(&[u8]) -> Result<serde_json::Value, E>,
) -> Result<serde_json::Value, RowBlobDecodeError<E>> {
    let mut frame = Vec::new();
    let input = if matches!(
        tag,
        TYPE_STRING
            | TYPE_JSON
            | TYPE_VECTOR_F32
            | TYPE_DATE
            | TYPE_TIME
            | TYPE_TIMESTAMP
            | TYPE_BYTEA
            | TYPE_EXTENDED
            | TYPE_COMPOSITE
            | TYPE_DOMAIN
            | TYPE_ENUM
    ) {
        write_varint(bytes.len() as u64, &mut frame);
        frame.extend_from_slice(bytes);
        frame.as_slice()
    } else {
        bytes
    };
    let mut cursor = Cursor::new(input);
    let value = decode_value(tag, &mut cursor, decode_json)?;
    if !cursor.remaining().is_empty() {
        return Err(invalid("row blob field payload has unconsumed bytes").into());
    }
    Ok(value)
}

fn decode_value<E>(
    tag: u8,
    cursor: &mut Cursor<'_>,
    decode_json: &impl Fn(&[u8]) -> Result<serde_json::Value, E>,
) -> Result<serde_json::Value, RowBlobDecodeError<E>> {
    match tag {
        TYPE_NULL => Ok(serde_json::Value::Null),
        TYPE_BOOL => Ok(serde_json::Value::Bool(cursor.read_u8()? != 0)),
        TYPE_I64 => Ok(serde_json::Value::Number(cursor.read_i64()?.into())),
        TYPE_F64 => serde_json::Number::from_f64(cursor.read_f64()?)
            .map(serde_json::Value::Number)
            .ok_or_else(|| invalid("invalid f64 in row blob").into()),
        TYPE_UUID => {
            let uuid = uuid::Uuid::from_slice(cursor.read_exact(16)?)
                .map_err(|error| invalid(format!("invalid UUID in row blob: {error}")))?;
            Ok(serde_json::Value::String(uuid.to_string()))
        }
        TYPE_STRING | TYPE_DATE | TYPE_TIME | TYPE_TIMESTAMP => {
            String::from_utf8(cursor.read_len_prefixed()?)
                .map(serde_json::Value::String)
                .map_err(|error| invalid(format!("invalid UTF-8 in row blob: {error}")).into())
        }
        TYPE_JSON => {
            let bytes = cursor.read_len_prefixed()?;
            decode_json(&bytes).map_err(RowBlobDecodeError::Json)
        }
        TYPE_BYTEA => Ok(serde_json::Value::String(encode_bytea(
            &cursor.read_len_prefixed()?,
        ))),
        TYPE_VECTOR_F32 => decode_vector(cursor),
        TYPE_ARRAY => decode_array(cursor, decode_json),
        TYPE_COMPOSITE => decode_composite(cursor, decode_json, 0),
        TYPE_DOMAIN => decode_domain(cursor, decode_json),
        TYPE_ENUM => decode_enum(cursor),
        TYPE_EXTENDED => {
            let bytes = cursor.read_len_prefixed()?;
            let (oid, payload) = bytes
                .split_first_chunk::<4>()
                .ok_or_else(|| invalid("truncated typed value in row blob"))?;
            let kind = ExtendedType::for_oid(u32::from_be_bytes(*oid))
                .ok_or_else(|| invalid("unknown typed value in row blob"))?;
            kind.receive_binary(payload)
                .map(|typed| typed.to_json())
                .map_err(|error| invalid(error.clone()).into())
        }
        _ => Err(invalid(format!("unsupported row blob type tag {tag}")).into()),
    }
}

fn decode_array<E>(
    cursor: &mut Cursor<'_>,
    decode_json: &impl Fn(&[u8]) -> Result<serde_json::Value, E>,
) -> Result<serde_json::Value, RowBlobDecodeError<E>> {
    let dimension_count = usize::try_from(cursor.read_varint()?)
        .map_err(|_| invalid("array dimension count out of range in row blob"))?;
    if dimension_count > cursor.remaining().len() / 8 {
        return Err(invalid("array dimension count exceeds remaining payload in row blob").into());
    }
    let mut dimensions = Vec::with_capacity(dimension_count);
    for _ in 0..dimension_count {
        let length = i32::from_be_bytes(
            cursor
                .read_exact(4)?
                .try_into()
                .map_err(|_| invalid("invalid array dimension length in row blob"))?,
        );
        let lower_bound = i32::from_be_bytes(
            cursor
                .read_exact(4)?
                .try_into()
                .map_err(|_| invalid("invalid array lower bound in row blob"))?,
        );
        dimensions.push(ArrayDimension {
            length,
            lower_bound,
        });
    }
    let count = usize::try_from(cursor.read_varint()?)
        .map_err(|_| invalid("array length out of range in row blob"))?;
    if count > cursor.remaining().len() / 2 {
        return Err(invalid("array count exceeds remaining payload in row blob").into());
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let element_tag = cursor.read_u8()?;
        let value = match element_tag {
            TYPE_NULL => {
                if cursor.read_varint()? != 0 {
                    return Err(
                        invalid("null array element carries a nonzero payload length").into(),
                    );
                }
                Value::Null
            }
            TYPE_ARRAY => {
                return Err(invalid("nested array element is invalid in CRB1 row blob").into())
            }
            TYPE_EXTENDED => decode_array_extended(cursor)?,
            _ => value_from_decoded_tag(
                element_tag,
                decode_value(element_tag, cursor, decode_json)?,
            )?,
        };
        values.push(value);
    }
    SqlArray::new(dimensions, values)
        .map(|array| array.to_storage_json())
        .map_err(|error| invalid(error).into())
}

fn decode_array_extended(cursor: &mut Cursor<'_>) -> Result<Value, StorageCodecError> {
    let bytes = cursor.read_len_prefixed()?;
    let (oid, payload) = bytes
        .split_first_chunk::<4>()
        .ok_or_else(|| invalid("truncated typed array value in row blob"))?;
    let kind = ExtendedType::for_oid(u32::from_be_bytes(*oid))
        .ok_or_else(|| invalid("unknown typed array value in row blob"))?;
    kind.receive_binary(payload)
        .map(Value::Typed)
        .map_err(|error| invalid(error.clone()))
}

fn value_from_decoded_tag<E>(
    tag: u8,
    value: serde_json::Value,
) -> Result<Value, RowBlobDecodeError<E>> {
    match tag {
        TYPE_BOOL => value.as_bool().map(Value::Bool),
        TYPE_I64 => value.as_i64().map(Value::Int64),
        TYPE_F64 => value.as_f64().map(Value::Float64),
        TYPE_STRING | TYPE_DATE | TYPE_TIME | TYPE_TIMESTAMP | TYPE_BYTEA | TYPE_UUID => {
            value.as_str().map(|text| Value::String(text.to_owned()))
        }
        TYPE_VECTOR_F32 => value
            .as_array()
            .and_then(|items| {
                items
                    .iter()
                    .map(|item| {
                        item.as_f64()
                            .and_then(|number| number.to_string().parse::<f32>().ok())
                    })
                    .collect::<Option<Vec<_>>>()
            })
            .map(|items| Value::Vector(cassie_core::types::Vector::new(items))),
        TYPE_JSON | TYPE_ENUM | TYPE_COMPOSITE => Some(Value::Json(value)),
        _ => None,
    }
    .ok_or_else(|| invalid(format!("invalid array element for type tag {tag}")).into())
}

fn decode_composite<E>(
    cursor: &mut Cursor<'_>,
    decode_json: &impl Fn(&[u8]) -> Result<serde_json::Value, E>,
    depth: usize,
) -> Result<serde_json::Value, RowBlobDecodeError<E>> {
    if depth >= 32 {
        return Err(invalid("composite value nests too deeply in row blob").into());
    }
    let bytes = cursor.read_len_prefixed()?;
    let mut payload = Cursor::new(&bytes);
    let version = payload.read_u8()?;
    if version != 1 {
        return Err(invalid(format!("unsupported composite payload version {version}")).into());
    }
    let oid = payload.read_u32()?;
    let revision = u64::from_be_bytes(
        payload
            .read_exact(8)?
            .try_into()
            .map_err(|_| invalid("invalid composite descriptor revision in row blob"))?,
    );
    let count = usize::try_from(payload.read_u32()?)
        .map_err(|_| invalid("composite field count out of range"))?;
    if count > payload.remaining().len() / 2 {
        return Err(invalid("composite field count exceeds remaining payload in row blob").into());
    }
    let mut fields = Vec::with_capacity(count);
    for _ in 0..count {
        let tag = payload.read_u8()?;
        let value = match tag {
            TYPE_NULL => {
                if payload.read_varint()? != 0 {
                    return Err(
                        invalid("null composite field carries a nonzero payload length").into(),
                    );
                }
                serde_json::Value::Null
            }
            TYPE_COMPOSITE => decode_composite(&mut payload, decode_json, depth + 1)?,
            other => decode_value(other, &mut payload, decode_json)?,
        };
        fields.push(value);
    }
    if !payload.remaining().is_empty() {
        return Err(invalid("composite payload has unconsumed bytes").into());
    }
    let mut body = serde_json::Map::new();
    body.insert("oid".into(), oid.into());
    body.insert("revision".into(), revision.into());
    body.insert("fields".into(), fields.into());
    let mut record = serde_json::Map::new();
    record.insert(
        cassie_core::types::composite::RECORD_STORAGE_KEY.into(),
        body.into(),
    );
    Ok(record.into())
}

fn decode_domain<E>(
    cursor: &mut Cursor<'_>,
    decode_json: &impl Fn(&[u8]) -> Result<serde_json::Value, E>,
) -> Result<serde_json::Value, RowBlobDecodeError<E>> {
    let bytes = cursor.read_len_prefixed()?;
    let mut payload = Cursor::new(&bytes);
    let version = payload.read_u8()?;
    if version != 1 {
        return Err(invalid(format!("unsupported domain payload version {version}")).into());
    }
    payload.read_u32()?;
    payload.read_exact(8)?;
    let tag = payload.read_u8()?;
    if tag == TYPE_NULL {
        return Err(invalid("domain payload wraps a NULL base value").into());
    }
    let value = decode_value(tag, &mut payload, decode_json)?;
    if !payload.remaining().is_empty() {
        return Err(invalid("domain payload has unconsumed bytes").into());
    }
    Ok(value)
}

fn decode_enum<E>(cursor: &mut Cursor<'_>) -> Result<serde_json::Value, RowBlobDecodeError<E>> {
    let bytes = cursor.read_len_prefixed()?;
    let mut payload = Cursor::new(&bytes);
    let version = payload.read_u8()?;
    if version != 1 {
        return Err(invalid(format!("unsupported enum payload version {version}")).into());
    }
    payload.read_u32()?;
    let member_id = u64::from_be_bytes(
        payload
            .read_exact(8)?
            .try_into()
            .map_err(|_| invalid("invalid enum member id in row blob"))?,
    );
    if !payload.remaining().is_empty() {
        return Err(invalid("enum payload has unconsumed bytes").into());
    }
    Ok(serde_json::json!({ cassie_core::types::enum_type::ENUM_STORAGE_KEY: member_id }))
}

fn decode_vector<E>(cursor: &mut Cursor<'_>) -> Result<serde_json::Value, RowBlobDecodeError<E>> {
    let bytes = cursor.read_len_prefixed()?;
    let Some((header, components)) = bytes.split_first_chunk::<4>() else {
        return Err(invalid("invalid vector field in row blob").into());
    };
    let dimensions = usize::try_from(u32::from_be_bytes(*header))
        .map_err(|_| invalid("invalid vector dimension in row blob"))?;
    if dimensions.checked_mul(4) != Some(components.len()) {
        return Err(invalid("invalid vector length in row blob").into());
    }
    let mut values: Vec<serde_json::Value> = Vec::with_capacity(dimensions);
    for chunk in components.as_chunks::<4>().0 {
        let value = f32::from_be_bytes(*chunk);
        let number = serde_json::Number::from_f64(f64::from(value))
            .ok_or_else(|| invalid("invalid vector numeric value in row blob"))?;
        values.push(number.into());
    }
    Ok(values.into())
}

fn check_integer_width(
    data_type: &DataType,
    value: &serde_json::Value,
) -> Result<(), StorageCodecError> {
    let valid = match data_type {
        DataType::SmallInt => value.as_i64().is_some_and(|n| i16::try_from(n).is_ok()),
        DataType::Int => value.as_i64().is_some_and(|n| i32::try_from(n).is_ok()),
        DataType::ObjectId(_) => value.as_i64().is_some_and(|n| u32::try_from(n).is_ok()),
        _ => true,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid(format!(
            "stored integer is outside declared {} range",
            data_type.type_name()
        )))
    }
}

fn corrupt<E>(message: String) -> RowBlobDecodeError<E> {
    RowBlobDecodeError::DataCorruption {
        sqlstate: "XX001",
        message,
    }
}

fn check_column<E>(
    data_type: &DataType,
    value: &serde_json::Value,
) -> Result<(), RowBlobDecodeError<E>> {
    if let DataType::Composite(descriptor) = data_type {
        check_composite(descriptor, value)?;
    }
    match data_type {
        DataType::Enum(descriptor) => check_enum(descriptor, value),
        DataType::Array(element) if matches!(element.as_ref(), DataType::Enum(_)) => {
            let DataType::Enum(descriptor) = element.as_ref() else {
                return Ok(());
            };
            let array = SqlArray::from_storage_json(value).map_err(|detail| {
                corrupt(format!(
                    "stored enum value does not match type {}: {detail}",
                    descriptor.name
                ))
            })?;
            for item in array.elements() {
                match item {
                    Value::Null => {}
                    Value::Json(document) => check_enum(descriptor, document)?,
                    _ => return Err(corrupt(format!("stored enum value does not match type {}: array element is not an enum document", descriptor.name))),
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn check_enum<E>(
    descriptor: &cassie_core::types::EnumType,
    value: &serde_json::Value,
) -> Result<(), RowBlobDecodeError<E>> {
    match cassie_core::types::EnumValue::storage_member_id(value) {
        Some(member_id) if descriptor.member(member_id).is_some() => Ok(()),
        Some(member_id) => Err(corrupt(format!(
            "stored enum value does not match type {}: unknown member id {member_id}",
            descriptor.name
        ))),
        None => Err(corrupt(format!(
            "stored enum value does not match type {}: not an enum document",
            descriptor.name
        ))),
    }
}

fn check_composite<E>(
    descriptor: &cassie_core::types::composite::CompositeType,
    value: &serde_json::Value,
) -> Result<(), RowBlobDecodeError<E>> {
    let corrupt = |detail: String| {
        corrupt(format!(
            "stored composite value does not match type {}: {detail}",
            descriptor.name
        ))
    };
    let body = value
        .as_object()
        .and_then(|object| object.get(cassie_core::types::composite::RECORD_STORAGE_KEY))
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| corrupt("not a record document".to_owned()))?;
    let oid = body.get("oid").and_then(serde_json::Value::as_u64);
    if oid != Some(u64::from(descriptor.oid)) {
        return Err(corrupt(format!(
            "type identity {oid:?} differs from {}",
            descriptor.oid
        )));
    }
    let slots = body
        .get("fields")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| corrupt("missing field list".to_owned()))?;
    if slots.len() > descriptor.physical_len() {
        return Err(corrupt(format!(
            "{} fields stored but the type has {}",
            slots.len(),
            descriptor.physical_len()
        )));
    }
    for (field, slot) in descriptor
        .fields
        .iter()
        .zip(descriptor.physical_positions())
    {
        if let (DataType::Composite(nested), Some(stored)) = (&field.data_type, slots.get(slot)) {
            if !stored.is_null() {
                check_composite(nested, stored)?;
            }
        }
    }
    Ok(())
}

#[derive(Debug, PartialEq)]
pub enum CompactScalar<'a> {
    Null,
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Text(&'a str),
    Generic(Option<serde_json::Value>),
}

/// # Errors
/// Returns a codec error for malformed or schema-inconsistent row data, or forwards an error from the JSON decoder.
pub fn decode_compact_scalar<'a, E>(
    data_type: &DataType,
    bytes: &'a [u8],
    decode_json: impl Fn(&[u8]) -> Result<serde_json::Value, E>,
) -> Result<CompactScalar<'a>, RowBlobDecodeError<E>> {
    let native = matches!(
        data_type,
        DataType::SmallInt
            | DataType::Int
            | DataType::BigInt
            | DataType::Float
            | DataType::Boolean
            | DataType::Text
            | DataType::Varchar { .. }
            | DataType::Char { .. }
    );
    if !native {
        return decode_compact_field_value(data_type, bytes, decode_json)
            .map(CompactScalar::Generic);
    }
    let (&tag, payload) = bytes
        .split_first()
        .ok_or_else(|| invalid("empty compact value"))?;
    if tag == TYPE_NULL {
        if !payload.is_empty() {
            return Err(invalid("row blob field payload has unconsumed bytes").into());
        }
        return Ok(CompactScalar::Null);
    }
    let expected = expected_type_tag(data_type);
    if tag != expected {
        return Err(invalid(format!(
            "stored type tag {tag} but the declared type uses type tag {expected}"
        ))
        .into());
    }
    let eight = |what: &str| {
        <[u8; 8]>::try_from(payload).map_err(|_| {
            invalid(if payload.len() > 8 {
                "row blob field payload has unconsumed bytes".to_owned()
            } else {
                format!("invalid {what} in row blob")
            })
        })
    };
    match data_type {
        DataType::Float => {
            let value = f64::from_be_bytes(eight("f64")?);
            if value.is_finite() {
                Ok(CompactScalar::Float(value))
            } else {
                Err(invalid("invalid f64 in row blob").into())
            }
        }
        DataType::Boolean => match payload {
            [flag] => Ok(CompactScalar::Boolean(*flag != 0)),
            [] => Err(invalid("invalid bool in row blob").into()),
            _ => Err(invalid("row blob field payload has unconsumed bytes").into()),
        },
        DataType::Text | DataType::Varchar { .. } | DataType::Char { .. } => {
            std::str::from_utf8(payload)
                .map(CompactScalar::Text)
                .map_err(|error| invalid(format!("invalid UTF-8 in row blob: {error}")).into())
        }
        _ => {
            let value = i64::from_be_bytes(eight("i64")?);
            let fits = match data_type {
                DataType::SmallInt => i16::try_from(value).is_ok(),
                DataType::Int => i32::try_from(value).is_ok(),
                _ => true,
            };
            if fits {
                Ok(CompactScalar::Integer(value))
            } else {
                Err(invalid(format!(
                    "stored integer is outside declared {} range",
                    data_type.type_name()
                ))
                .into())
            }
        }
    }
}

#[must_use]
pub fn encode_compact_sql_null() -> Vec<u8> {
    vec![TYPE_NULL]
}

#[derive(Debug, Clone)]
pub struct VectorColumn {
    field_index: usize,
}

impl VectorColumn {
    #[must_use]
    pub fn resolve(schema: &RowSchema, field_name: &str) -> Option<Self> {
        let requested = ColumnIdentifierPath::from_field_name(field_name).lookup_key();
        let active = schema
            .fields
            .iter()
            .filter(|field| !field.retired)
            .map(|field| ColumnIdentifierPath::from_field_name(&field.name).lookup_key())
            .collect::<HashSet<_>>();
        let field_index = schema.fields.iter().position(|field| {
            ((!field.retired
                && ColumnIdentifierPath::from_field_name(&field.name).lookup_key() == requested)
                || (field.aliases.iter().any(|alias| {
                    ColumnIdentifierPath::from_field_name(alias).lookup_key() == requested
                }) && !active.contains(&requested)))
                && matches!(field.data_type, DataType::Vector(_))
        })?;
        Some(Self { field_index })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PackedVector<'a> {
    components: &'a [u8],
}

impl PackedVector<'_> {
    #[must_use]
    pub const fn dimensions(&self) -> usize {
        self.components.len() / 4
    }
    #[must_use]
    pub const fn decoded_bytes(&self) -> usize {
        self.components.len()
    }
    /// # Errors
    /// Returns a codec error when packed vector bytes contain a non-finite component.
    pub fn to_f32(self) -> Result<Vec<f32>, StorageCodecError> {
        let mut values = Vec::with_capacity(self.dimensions());
        for chunk in self.components.as_chunks::<4>().0 {
            let value = f32::from_be_bytes(*chunk);
            if !value.is_finite() {
                return Err(invalid("invalid vector numeric value in row blob"));
            }
            values.push(value);
        }
        Ok(values)
    }
}

pub struct VectorRowParts<'a> {
    pub vector: Option<PackedVector<'a>>,
    pub payload: Option<serde_json::Value>,
}

/// # Errors
/// Returns a codec error for malformed or schema-inconsistent row data, or forwards an error from the JSON decoder.
pub fn split_vector_row<'a, E>(
    schema: &RowSchema,
    column: &VectorColumn,
    row: &'a [u8],
    with_payload: bool,
    decode_json: impl Fn(&[u8]) -> Result<serde_json::Value, E>,
) -> Result<VectorRowParts<'a>, RowBlobDecodeError<E>> {
    let record = RowBlobRecord::parse(row)?;
    let vector_field = schema
        .fields
        .get(column.field_index)
        .ok_or_else(|| invalid("row vector field index is out of bounds"))?;
    let vector = match record.field(vector_field.field_id)? {
        None | Some(RowBlobField::SqlNull) => None,
        Some(RowBlobField::Encoded { type_tag, bytes }) => {
            if type_tag != TYPE_VECTOR_F32 {
                return Err(invalid(format!("row blob field '{}' stores type tag {type_tag} but its schema declares type tag {TYPE_VECTOR_F32}", vector_field.name)).into());
            }
            let Some((header, components)) = bytes.split_first_chunk::<4>() else {
                return Err(invalid("invalid vector field in row blob").into());
            };
            let dimensions = usize::try_from(u32::from_be_bytes(*header))
                .map_err(|_| invalid("invalid vector dimension in row blob"))?;
            if dimensions.checked_mul(4) != Some(components.len()) {
                return Err(invalid("invalid vector length in row blob").into());
            }
            Some(PackedVector { components })
        }
    };
    let payload = if with_payload {
        let mut object = serde_json::Map::new();
        for field in &schema.fields {
            if field.retired || field.field_id == vector_field.field_id {
                continue;
            }
            if let Some(value) = decode_field(&record, field, &decode_json)? {
                object.insert(field.name.clone(), value);
            }
        }
        Some(object.into())
    } else {
        None
    };
    Ok(VectorRowParts { vector, payload })
}

pub(super) fn encode_bytea(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(2 + bytes.len() * 2);
    encoded.push_str("\\x");
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn write_varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        let byte = u8::try_from(value & 0x7f).expect("masked to 7 bits");
        out.push(byte | 0x80);
        value >>= 7;
    }
    out.push(u8::try_from(value).expect("final varint byte fits in u8"));
}
