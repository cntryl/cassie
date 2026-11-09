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
#[path = "input/binary_date_time_bounds_tests.rs"]
mod binary_date_time_bounds_tests;

#[cfg(test)]
#[path = "input/binary_timestamp_bounds_tests.rs"]
mod binary_timestamp_bounds_tests;

#[cfg(test)]
#[path = "input/binary_timetz_bounds_tests.rs"]
mod binary_timetz_bounds_tests;

const BINARY_SIGNATURE: &[u8; 11] = b"PGCOPY\n\xff\r\n\0";

pub(super) fn parse_text_payload(
    payload: &[u8],
    delimiter: char,
    null_marker: &str,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<
    (
        Vec<Vec<Option<String>>>,
        crate::runtime::QueryMemoryReservation,
    ),
    CassieError,
> {
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
) -> Result<
    (
        Vec<Vec<Option<String>>>,
        crate::runtime::QueryMemoryReservation,
    ),
    super::filter::CopyWhereError,
> {
    controls.check_interruption()?;
    let schema = cassie
        .catalog
        .get_schema(&statement.table)
        .ok_or_else(|| CassieError::CollectionNotFound(statement.table.clone()))?;
    let _type_memory;
    let column_types;
    (column_types, _type_memory) =
        copy_column_types(statement, schema.fields.as_slice(), controls)?;
    drop(schema);
    let mut cursor = 0_usize;
    let signature = take(payload, &mut cursor, BINARY_SIGNATURE.len())?;
    if signature != BINARY_SIGNATURE {
        return Err(crate::app::DiagnosticError::new(
            "22P04",
            "COPY file signature not recognized",
        )
        .into());
    }
    let flags = i32::from_be_bytes(take(payload, &mut cursor, 4)?.try_into().map_err(|_| {
        CassieError::Parse("COPY binary payload has a truncated flags field".to_string())
    })?);
    if flags != 0 {
        return Err(
            CassieError::Unsupported("COPY binary flags are not supported".to_string()).into(),
        );
    }
    let extension_length = i32::from_be_bytes(
        take(payload, &mut cursor, 4)?
            .try_into()
            .map_err(|_| CassieError::Parse("COPY binary header is truncated".to_string()))?,
    );
    let extension_length = usize::try_from(extension_length)
        .map_err(|_| CassieError::Parse("COPY binary extension length is negative".into()))?;
    let _extension = take(payload, &mut cursor, extension_length)?;

    let mut memory = controls.reserve_query_memory(0)?;
    let mut rows = Vec::new();
    loop {
        controls.check_interruption()?;
        let field_count = i16::from_be_bytes(
            take(payload, &mut cursor, 2)?
                .try_into()
                .map_err(|_| CassieError::Parse("COPY binary row header is truncated".into()))?,
        );
        if field_count == -1 {
            if cursor != payload.len() {
                return Err(CassieError::Parse(
                    "COPY binary payload has trailing bytes after its trailer".into(),
                )
                .into());
            }
            return Ok((rows, memory));
        }
        if field_count < 0 {
            return Err(CassieError::Parse(
                "COPY binary row has an invalid field count".to_string(),
            )
            .into());
        }
        let expected = i16::try_from(column_types.len())
            .map_err(|_| CassieError::ResourceLimit("COPY column count exceeds int16".into()))?;
        if field_count != expected {
            return Err(CassieError::Parse(format!(
                "COPY binary row has {field_count} columns but target expects {expected}"
            ))
            .into());
        }
        let mut row_memory = controls.reserve_query_memory(0)?;
        let mut row = Vec::new();
        for (column_index, type_oid) in column_types.iter().copied().enumerate() {
            controls.check_interruption()?;
            crate::app::copy_decode::grow_text_vec(&mut row, &mut row_memory, controls)?;
            let length =
                i32::from_be_bytes(take(payload, &mut cursor, 4)?.try_into().map_err(|_| {
                    CassieError::Parse("COPY binary field length is truncated".into())
                })?);
            if length == -1 {
                row.push(None);
                continue;
            }
            let length = usize::try_from(length)
                .map_err(|_| CassieError::Parse("COPY binary field length is negative".into()))?;
            let bytes = take(payload, &mut cursor, length)?;
            if let Some((text, text_memory)) =
                binary_bit::decode(type_oid, bytes, column_index, controls)?
            {
                row.push(Some(text));
                row_memory.absorb(text_memory);
                continue;
            }
            if let Some((text, text_memory)) =
                binary_date::decode(type_oid, bytes, column_index, controls)?
            {
                row.push(Some(text));
                row_memory.absorb(text_memory);
                continue;
            }
            if let Some((text, text_memory)) =
                binary_fixed::decode(type_oid, bytes, column_index, controls)?
            {
                row.push(Some(text));
                row_memory.absorb(text_memory);
                continue;
            }
            if let Some((text, text_memory)) =
                binary_float8::decode(type_oid, bytes, column_index, controls)?
            {
                row.push(Some(text));
                row_memory.absorb(text_memory);
                continue;
            }
            if let Some((text, text_memory)) =
                binary_interval::decode(type_oid, bytes, column_index, controls)?
            {
                row.push(Some(text));
                row_memory.absorb(text_memory);
                continue;
            }
            if let Some((text, text_memory)) =
                binary_inet::decode(type_oid, bytes, column_index, controls)?
            {
                row.push(Some(text));
                row_memory.absorb(text_memory);
                continue;
            }
            if let Some((text, text_memory)) =
                binary_network::decode(type_oid, bytes, column_index, controls)?
            {
                row.push(Some(text));
                row_memory.absorb(text_memory);
                continue;
            }
            if let Some((text, text_memory)) =
                binary_name::decode(type_oid, bytes, column_index, controls)?
            {
                row.push(Some(text));
                row_memory.absorb(text_memory);
                continue;
            }
            if let Some((text, text_memory)) =
                binary_time::decode(type_oid, bytes, column_index, controls)?
            {
                row.push(Some(text));
                row_memory.absorb(text_memory);
                continue;
            }
            if let Some((text, text_memory)) =
                binary_timetz::decode(type_oid, bytes, column_index, controls)?
            {
                row.push(Some(text));
                row_memory.absorb(text_memory);
                continue;
            }
            if let Some((text, text_memory)) =
                binary_timestamp::decode(type_oid, bytes, column_index, controls)?
            {
                row.push(Some(text));
                row_memory.absorb(text_memory);
                continue;
            }
            if let Some((text, text_memory)) =
                binary_vector::decode(type_oid, bytes, column_index, controls)?
            {
                row.push(Some(text));
                row_memory.absorb(text_memory);
                continue;
            }
            if let Some((text, text_memory)) =
                binary_uuid::decode(type_oid, bytes, column_index, controls)?
            {
                row.push(Some(text));
                row_memory.absorb(text_memory);
                continue;
            }
            if let Some((text, text_memory)) =
                binary_numeric::decode(type_oid, bytes, column_index, controls)?
            {
                row.push(Some(text));
                row_memory.absorb(text_memory);
                continue;
            }
            if let Some((text, text_memory)) =
                binary_real::decode(type_oid, bytes, column_index, controls)?
            {
                row.push(Some(text));
                row_memory.absorb(text_memory);
                continue;
            }
            if let Some((text, text_memory)) =
                binary_text::decode(type_oid, bytes, column_index, controls)?
            {
                row.push(Some(text));
                row_memory.absorb(text_memory);
                continue;
            }
            if type_oid == 114 {
                use crate::runtime::accounted::json::controlled_parse::ParseError;
                let owner =
                    match crate::runtime::accounted::json::controlled_parse::parse(bytes, controls)
                    {
                        Ok(owner) => owner,
                        Err(ParseError::Controlled(error)) => return Err(error.into()),
                        Err(ParseError::InvalidJson {
                            error,
                            _memory: memory,
                        }) => {
                            drop(error);
                            drop(memory);
                            return Err(CassieError::Parse(format!(
                                "COPY binary field {} is invalid: invalid binary json value",
                                column_index + 1
                            ))
                            .into());
                        }
                        Err(ParseError::InvalidUtf8) => {
                            return Err(CassieError::Parse(format!(
                                "COPY binary field {} is invalid: invalid binary json value",
                                column_index + 1
                            ))
                            .into());
                        }
                    };
                let output =
                    crate::runtime::accounted::compact_json::serialize(owner.get(), controls)?;
                let (text, text_memory) = output.into_parts();
                row.push(Some(text));
                row_memory.absorb(text_memory);
                continue;
            }
            if type_oid == 3802 {
                use crate::runtime::accounted::json_arena::{parse_jsonb_field, JsonbFieldError};
                let output = match parse_jsonb_field(bytes, column_index + 1, controls) {
                    Ok(output) => output,
                    Err(JsonbFieldError::Control(error)) => return Err(error.into()),
                    Err(JsonbFieldError::Diagnostic(owner)) => {
                        return Err(super::filter::CopyWhereError::Jsonb(owner))
                    }
                };
                let (text, text_memory) = output.into_parts();
                row.push(Some(text));
                row_memory.absorb(text_memory);
                continue;
            }
            if type_oid == 3807 {
                use crate::runtime::accounted::json_arena::jsonb_array_output::{
                    render, OutputError,
                };
                let output = match render(bytes, column_index + 1, controls) {
                    Ok(output) => output,
                    Err(OutputError::Control(error)) => return Err(error.into()),
                    Err(OutputError::Diagnostic(owner)) => {
                        return Err(super::filter::CopyWhereError::Jsonb(owner))
                    }
                };
                let (text, text_memory) = output.into_parts();
                row.push(Some(text));
                row_memory.absorb(text_memory);
                continue;
            }
            let value =
                super::super::codecs::binary_to_value(bytes, type_oid).map_err(|error| {
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
                Value::Null => row.push(None),
                value => {
                    // Existing codec scratch is not yet pre-admitted; retain the
                    // resulting text under the COPY tracker once its size is known.
                    let text_memory;
                    let text;
                    text = super::super::codecs::value_to_text(&value, type_oid);
                    text_memory = controls.reserve_query_memory(text.capacity())?;
                    row.push(Some(text));
                    row_memory.absorb(text_memory);
                }
            }
        }
        crate::app::copy_decode::grow_text_vec(&mut rows, &mut memory, controls)?;
        rows.push(row);
        memory.absorb(row_memory);
    }
}

fn copy_column_types(
    statement: &CopyStatement,
    fields: &[crate::catalog::FieldMeta],
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<(Vec<i64>, crate::runtime::QueryMemoryReservation), CassieError> {
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
            types.push(field.data_type.type_oid());
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
            types.push(DataType::Text.type_oid());
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
        types.push(field.data_type.type_oid());
    }
    Ok((types, memory))
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
