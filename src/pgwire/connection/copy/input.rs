use crate::app::{Cassie, CassieError};
use crate::sql::ast::CopyStatement;
use crate::types::{DataType, Value};

#[path = "input/binary_bit.rs"]
mod binary_bit;
#[path = "input/binary_date.rs"]
mod binary_date;
#[path = "input/binary_fixed.rs"]
mod binary_fixed;
#[path = "input/binary_float8.rs"]
mod binary_float8;
#[path = "input/binary_inet.rs"]
mod binary_inet;
#[path = "input/binary_interval.rs"]
mod binary_interval;
#[cfg(test)]
#[path = "input/binary_jsonb_owner_tests.rs"]
mod binary_jsonb_owner_tests;
#[path = "input/binary_name.rs"]
mod binary_name;
#[path = "input/binary_network.rs"]
mod binary_network;
#[path = "input/binary_numeric.rs"]
mod binary_numeric;
#[path = "input/binary_real.rs"]
mod binary_real;
#[path = "input/binary_text.rs"]
mod binary_text;
#[path = "input/binary_time.rs"]
mod binary_time;
#[path = "input/binary_timestamp.rs"]
mod binary_timestamp;
#[path = "input/binary_timetz.rs"]
mod binary_timetz;
#[path = "input/binary_uuid.rs"]
mod binary_uuid;
#[path = "input/binary_vector.rs"]
mod binary_vector;

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "input/binary_json_owner_tests.rs"]
mod binary_json_owner_tests;

#[cfg(test)]
#[path = "input/binary_interval_bounds_tests.rs"]
mod binary_interval_bounds_tests;

#[cfg(test)]
#[path = "input/binary_date_time_bounds_tests.rs"]
mod binary_date_time_bounds_tests;

#[cfg(test)]
#[path = "input/binary_timestamp_bounds_tests.rs"]
mod binary_timestamp_bounds_tests;

#[cfg(test)]
#[path = "input/binary_timetz_bounds_tests.rs"]
mod binary_timetz_bounds_tests;

const BINARY_SIGNATURE: &[u8; 11] = b"PGCOPY\n\xff\r\n\0";

type CopyRowsWithMemory = (
    Vec<Vec<Option<String>>>,
    crate::runtime::QueryMemoryReservation,
);

type CopyRowWithMemory = (Vec<Option<String>>, crate::runtime::QueryMemoryReservation);
type CopyFieldText = (String, crate::runtime::QueryMemoryReservation);

type BinaryFieldDecoder = fn(
    i64,
    &[u8],
    usize,
    &crate::runtime::QueryExecutionControls,
) -> Result<Option<CopyFieldText>, CassieError>;

enum DecodedBinaryField {
    Null,
    Text(String, crate::runtime::QueryMemoryReservation),
}

#[derive(Clone, Copy)]
struct CopyColumnType {
    oid: i64,
    is_vector: bool,
    expected_vector_dimensions: Option<usize>,
}

pub(super) fn parse_text_payload(
    payload: &[u8],
    delimiter: char,
    null_marker: &str,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<CopyRowsWithMemory, CassieError> {
    crate::app::copy_decode::validate_payload_utf8(payload, controls)?;
    let mut memory = controls.reserve_query_memory(0)?;
    let mut row_memory = controls.reserve_query_memory(0)?;
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut delimiter_bytes = [0_u8; 4];
    let delimiter = delimiter.encode_utf8(&mut delimiter_bytes).as_bytes();
    let mut field_start = 0;
    let mut index = 0;
    let mut next_check = 0;
    while index < payload.len() {
        if index >= next_check {
            controls.check_interruption()?;
            next_check = index.saturating_add(4096);
        }
        let delimiter_match = payload[index..].starts_with(delimiter);
        let newline = !delimiter_match && payload[index] == b'\n';
        let crlf =
            !delimiter_match && payload[index] == b'\r' && payload.get(index + 1) == Some(&b'\n');
        if delimiter_match || newline || crlf {
            push_text_field(
                &mut row,
                &mut row_memory,
                &payload[field_start..index],
                null_marker,
                controls,
            )?;
            index += if delimiter_match {
                delimiter.len()
            } else if crlf {
                2
            } else {
                1
            };
            field_start = index;
            if newline || crlf {
                crate::app::copy_decode::grow_text_vec(&mut rows, &mut memory, controls)?;
                rows.push(std::mem::take(&mut row));
                memory.absorb(row_memory);
                row_memory = controls.reserve_query_memory(0)?;
            }
        } else {
            index += 1;
        }
    }
    if field_start < payload.len() || !row.is_empty() {
        push_text_field(
            &mut row,
            &mut row_memory,
            &payload[field_start..],
            null_marker,
            controls,
        )?;
        crate::app::copy_decode::grow_text_vec(&mut rows, &mut memory, controls)?;
        rows.push(row);
        memory.absorb(row_memory);
    }
    Ok((rows, memory))
}

fn push_text_field(
    row: &mut Vec<Option<String>>,
    memory: &mut crate::runtime::QueryMemoryReservation,
    raw: &[u8],
    null_marker: &str,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<(), CassieError> {
    controls.check_interruption()?;
    crate::app::copy_decode::grow_text_vec(row, memory, controls)?;
    let mut is_null = raw.len() == null_marker.len();
    if is_null {
        for (left, right) in raw.chunks(4096).zip(null_marker.as_bytes().chunks(4096)) {
            controls.check_interruption()?;
            if left != right {
                is_null = false;
                break;
            }
        }
    }
    if is_null {
        row.push(None);
        return Ok(());
    }
    let field_memory = controls.reserve_query_memory(raw.len())?;
    let mut field = String::new();
    field
        .try_reserve_exact(raw.len())
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if field.capacity() > raw.len() {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY field capacity".into(),
        ));
    }
    let mut start = 0;
    let mut escaped = false;
    while start < raw.len() {
        controls.check_interruption()?;
        let mut end = (start + 4096).min(raw.len());
        while end > start && end < raw.len() && raw[end] & 0xc0 == 0x80 {
            end -= 1;
        }
        let chunk = std::str::from_utf8(&raw[start..end])
            .map_err(|_| CassieError::Parse("COPY payload must be UTF-8".into()))?;
        for character in chunk.chars() {
            if escaped {
                field.push(match character {
                    'b' => '\u{0008}',
                    'f' => '\u{000c}',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    'v' => '\u{000b}',
                    other => other,
                });
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else {
                field.push(character);
            }
        }
        start = end;
    }
    if escaped {
        field.push('\\');
    }
    memory.absorb(field_memory);
    row.push(Some(field));
    Ok(())
}

pub(super) fn parse_binary_payload(
    payload: &[u8],
    cassie: &Cassie,
    statement: &CopyStatement,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<CopyRowsWithMemory, super::filter::CopyWhereError> {
    parse_binary_payload_in_database(
        payload,
        cassie,
        &cassie.default_database,
        statement,
        controls,
    )
}

pub(super) fn parse_binary_payload_in_database(
    payload: &[u8],
    cassie: &Cassie,
    database: &str,
    statement: &CopyStatement,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<CopyRowsWithMemory, super::filter::CopyWhereError> {
    controls.check_interruption()?;
    let schema = cassie
        .catalog
        .get_schema(&statement.table)
        .ok_or_else(|| CassieError::CollectionNotFound(statement.table.clone()))?;
    let _type_memory;
    let column_types;
    (column_types, _type_memory) = copy_column_types_for_database(
        statement,
        schema.fields.as_slice(),
        Some((&cassie.catalog, database)),
        controls,
    )?;
    drop(schema);
    let mut cursor = 0_usize;
    parse_binary_header(payload, &mut cursor)?;
    parse_binary_rows(payload, &mut cursor, &column_types, controls)
}

fn parse_binary_header(
    payload: &[u8],
    cursor: &mut usize,
) -> Result<(), super::filter::CopyWhereError> {
    let signature = take(payload, cursor, BINARY_SIGNATURE.len())?;
    if signature != BINARY_SIGNATURE {
        return Err(crate::app::DiagnosticError::new(
            "22P04",
            "COPY file signature not recognized",
        )
        .into());
    }
    let flags = i32::from_be_bytes(take(payload, cursor, 4)?.try_into().map_err(|_| {
        CassieError::Parse("COPY binary payload has a truncated flags field".to_string())
    })?);
    if flags != 0 {
        return Err(
            CassieError::Unsupported("COPY binary flags are not supported".to_string()).into(),
        );
    }
    let extension_length = i32::from_be_bytes(
        take(payload, cursor, 4)?
            .try_into()
            .map_err(|_| CassieError::Parse("COPY binary header is truncated".to_string()))?,
    );
    let extension_length = usize::try_from(extension_length)
        .map_err(|_| CassieError::Parse("COPY binary extension length is negative".into()))?;
    let _extension = take(payload, cursor, extension_length)?;
    Ok(())
}

fn parse_binary_rows(
    payload: &[u8],
    cursor: &mut usize,
    column_types: &[CopyColumnType],
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<CopyRowsWithMemory, super::filter::CopyWhereError> {
    let mut memory = controls.reserve_query_memory(0)?;
    let mut rows = Vec::new();
    loop {
        controls.check_interruption()?;
        let Some(()) = validate_binary_row_header(payload, cursor, column_types.len())? else {
            return Ok((rows, memory));
        };
        let (row, row_memory) = parse_binary_row(payload, cursor, column_types, controls)?;
        crate::app::copy_decode::grow_text_vec(&mut rows, &mut memory, controls)?;
        rows.push(row);
        memory.absorb(row_memory);
    }
}

fn validate_binary_row_header(
    payload: &[u8],
    cursor: &mut usize,
    column_count: usize,
) -> Result<Option<()>, CassieError> {
    let field_count = i16::from_be_bytes(
        take(payload, cursor, 2)?
            .try_into()
            .map_err(|_| CassieError::Parse("COPY binary row header is truncated".into()))?,
    );
    if field_count == -1 {
        if *cursor != payload.len() {
            return Err(CassieError::Parse(
                "COPY binary payload has trailing bytes after its trailer".into(),
            ));
        }
        return Ok(None);
    }
    if field_count < 0 {
        return Err(CassieError::Parse(
            "COPY binary row has an invalid field count".to_string(),
        ));
    }
    let expected = i16::try_from(column_count)
        .map_err(|_| CassieError::ResourceLimit("COPY column count exceeds int16".into()))?;
    if field_count != expected {
        return Err(CassieError::Parse(format!(
            "COPY binary row has {field_count} columns but target expects {expected}"
        )));
    }
    Ok(Some(()))
}

fn parse_binary_row(
    payload: &[u8],
    cursor: &mut usize,
    column_types: &[CopyColumnType],
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<CopyRowWithMemory, super::filter::CopyWhereError> {
    let mut row_memory = controls.reserve_query_memory(0)?;
    let mut row = Vec::new();
    for (column_index, column_type) in column_types.iter().copied().enumerate() {
        controls.check_interruption()?;
        crate::app::copy_decode::grow_text_vec(&mut row, &mut row_memory, controls)?;
        let length = i32::from_be_bytes(
            take(payload, cursor, 4)?
                .try_into()
                .map_err(|_| CassieError::Parse("COPY binary field length is truncated".into()))?,
        );
        if length == -1 {
            row.push(None);
            continue;
        }
        let length = usize::try_from(length)
            .map_err(|_| CassieError::Parse("COPY binary field length is negative".into()))?;
        let bytes = take(payload, cursor, length)?;
        match decode_binary_field(column_type, bytes, column_index, controls)? {
            DecodedBinaryField::Null => row.push(None),
            DecodedBinaryField::Text(text, memory) => {
                row.push(Some(text));
                row_memory.absorb(memory);
            }
        }
    }
    Ok((row, row_memory))
}

fn decode_binary_field(
    column_type: CopyColumnType,
    bytes: &[u8],
    column_index: usize,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<DecodedBinaryField, super::filter::CopyWhereError> {
    if column_type.is_vector {
        let (text, memory) = binary_vector::decode(
            bytes,
            column_index,
            controls,
            column_type.expected_vector_dimensions,
        )?;
        return Ok(DecodedBinaryField::Text(text, memory));
    }
    let type_oid = column_type.oid;
    if let Some((text, memory)) =
        decode_special_binary_field(type_oid, bytes, column_index, controls)?
    {
        return Ok(DecodedBinaryField::Text(text, memory));
    }
    if let Some((text, memory)) = decode_json_binary_field(type_oid, bytes, column_index, controls)?
    {
        return Ok(DecodedBinaryField::Text(text, memory));
    }
    let value = super::super::codecs::binary_to_value(bytes, type_oid).map_err(|error| {
        if let Some(input) = super::super::codecs::timetz_copy_error(&error)
            .or_else(|| super::super::codecs::temporal_copy_error(&error))
        {
            return CassieError::TypeInput(input);
        }
        CassieError::Parse(format!(
            "COPY binary field {} is invalid: {error}",
            column_index + 1
        ))
    })?;
    match value {
        Value::Null => Ok(DecodedBinaryField::Null),
        value => {
            // Existing codec scratch is not yet pre-admitted; retain the
            // resulting text under the COPY tracker once its size is known.
            let text = super::super::codecs::value_to_text(&value, type_oid);
            let memory = controls.reserve_query_memory(text.capacity())?;
            Ok(DecodedBinaryField::Text(text, memory))
        }
    }
}

fn decode_special_binary_field(
    type_oid: i64,
    bytes: &[u8],
    column_index: usize,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<Option<CopyFieldText>, CassieError> {
    let decoders = [
        binary_bit::decode as BinaryFieldDecoder,
        binary_date::decode,
        binary_fixed::decode,
        binary_float8::decode,
        binary_interval::decode,
        binary_inet::decode,
        binary_network::decode,
        binary_name::decode,
        binary_time::decode,
        binary_timetz::decode,
        binary_timestamp::decode,
        binary_uuid::decode,
        binary_numeric::decode,
        binary_real::decode,
        binary_text::decode,
    ];
    for decode in decoders {
        if let Some(decoded) = decode(type_oid, bytes, column_index, controls)? {
            return Ok(Some(decoded));
        }
    }
    Ok(None)
}

fn decode_json_binary_field(
    type_oid: i64,
    bytes: &[u8],
    column_index: usize,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<Option<CopyFieldText>, super::filter::CopyWhereError> {
    let output = match type_oid {
        114 => {
            use crate::runtime::accounted::json_arena::{binary_json_text, BinaryJsonError};
            match binary_json_text(bytes, controls) {
                Ok(output) => output.into_parts(),
                Err(BinaryJsonError::Controlled(error)) => return Err(error.into()),
                Err(BinaryJsonError::Invalid) => {
                    return Err(CassieError::Parse(format!(
                        "COPY binary field {} is invalid: invalid binary json value",
                        column_index + 1
                    ))
                    .into());
                }
            }
        }
        3802 => {
            use crate::runtime::accounted::json_arena::{parse_jsonb_field, JsonbFieldError};
            match parse_jsonb_field(bytes, column_index + 1, controls) {
                Ok(output) => output.into_parts(),
                Err(JsonbFieldError::Control(error)) => return Err(error.into()),
                Err(JsonbFieldError::Diagnostic(owner)) => {
                    return Err(super::filter::CopyWhereError::Jsonb(owner));
                }
            }
        }
        3807 => {
            use crate::runtime::accounted::json_arena::jsonb_array_output::{render, OutputError};
            match render(bytes, column_index + 1, controls) {
                Ok(output) => output.into_parts(),
                Err(OutputError::Control(error)) => return Err(error.into()),
                Err(OutputError::Diagnostic(owner)) => {
                    return Err(super::filter::CopyWhereError::Jsonb(owner));
                }
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(output))
}

fn copy_column_types(
    statement: &CopyStatement,
    fields: &[crate::catalog::FieldMeta],
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<(Vec<CopyColumnType>, crate::runtime::QueryMemoryReservation), CassieError> {
    copy_column_types_for_database(statement, fields, None, controls)
}

fn copy_column_types_for_database(
    statement: &CopyStatement,
    fields: &[crate::catalog::FieldMeta],
    vector_context: Option<(&crate::catalog::Catalog, &str)>,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<(Vec<CopyColumnType>, crate::runtime::QueryMemoryReservation), CassieError> {
    let count = if statement.columns.is_empty() {
        fields.len()
    } else {
        statement.columns.len()
    };
    let base = controls.reserve_query_memory(0)?;
    let memory;
    let mut types;
    (types, memory) = crate::runtime::owned_memory::try_reserve_vec(count, &base)?;
    if statement.columns.is_empty() {
        for field in fields {
            controls.check_interruption()?;
            types.push(copy_column_type(&field.data_type, vector_context)?);
        }
        return Ok((types, memory));
    }
    let mut declares_id = false;
    for field in fields {
        controls.check_interruption()?;
        if crate::types::row_identity::is_legacy_id_column(&field.name) {
            declares_id = true;
            break;
        }
    }
    for column in &statement.columns {
        controls.check_interruption()?;
        if crate::types::row_identity::is_identity_reference(column, declares_id) {
            types.push(copy_column_type(&DataType::Text, vector_context)?);
            continue;
        }
        let mut matched = None;
        for field in fields {
            controls.check_interruption()?;
            if field.name.len() != column.len() {
                continue;
            }
            let mut equal = true;
            for (left, right) in field
                .name
                .as_bytes()
                .chunks(4096)
                .zip(column.as_bytes().chunks(4096))
            {
                controls.check_interruption()?;
                if left != right {
                    equal = false;
                    break;
                }
            }
            if equal {
                matched = Some(field);
                break;
            }
        }
        let field = matched.ok_or_else(|| {
            CassieError::Planner(format!(
                "COPY target column '{column}' does not exist in '{}'",
                statement.table
            ))
        })?;
        types.push(copy_column_type(&field.data_type, vector_context)?);
    }
    Ok((types, memory))
}

fn copy_column_type(
    data_type: &DataType,
    vector_context: Option<(&crate::catalog::Catalog, &str)>,
) -> Result<CopyColumnType, CassieError> {
    if let DataType::Vector(expected_vector_dimensions) = data_type {
        let oid = vector_context
            .and_then(|(catalog, database)| {
                crate::catalog::object_identity::type_oid_for_database(catalog, database, data_type)
            })
            .or_else(|| vector_context.is_none().then(|| data_type.type_oid()))
            .ok_or_else(|| {
                CassieError::Unsupported(
                    "vector type is not installed in the active database".into(),
                )
            })?;
        return Ok(CopyColumnType {
            oid,
            is_vector: true,
            expected_vector_dimensions: *expected_vector_dimensions,
        });
    }
    Ok(CopyColumnType {
        oid: data_type.type_oid(),
        is_vector: false,
        expected_vector_dimensions: None,
    })
}

fn take<'a>(payload: &'a [u8], cursor: &mut usize, length: usize) -> Result<&'a [u8], CassieError> {
    let end = cursor
        .checked_add(length)
        .ok_or_else(|| CassieError::ResourceLimit("COPY binary length overflow".into()))?;
    let bytes = payload
        .get(*cursor..end)
        .ok_or_else(|| CassieError::Parse("COPY binary payload is truncated".to_string()))?;
    *cursor = end;
    Ok(bytes)
}
