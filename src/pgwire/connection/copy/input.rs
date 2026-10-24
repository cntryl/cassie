use crate::app::{Cassie, CassieError};
use crate::sql::ast::CopyStatement;
use crate::types::{DataType, Value};

const BINARY_SIGNATURE: &[u8; 11] = b"PGCOPY\n\xff\r\n\0";

pub(super) fn parse_text_payload(
    payload: &[u8],
    delimiter: char,
    null_marker: &str,
) -> Result<Vec<Vec<Option<String>>>, CassieError> {
    let text = std::str::from_utf8(payload)
        .map_err(|_| CassieError::Parse("COPY payload must be UTF-8".to_string()))?;
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            character if character == delimiter => {
                row.push(decode_text_field(&field, null_marker));
                field.clear();
            }
            '\n' => {
                row.push(decode_text_field(&field, null_marker));
                field.clear();
                rows.push(std::mem::take(&mut row));
            }
            '\r' if characters.peek() == Some(&'\n') => {
                characters.next();
                row.push(decode_text_field(&field, null_marker));
                field.clear();
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(character),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(decode_text_field(&field, null_marker));
        rows.push(row);
    }
    Ok(rows)
}

pub(super) fn parse_binary_payload(
    payload: &[u8],
    cassie: &Cassie,
    statement: &CopyStatement,
) -> Result<Vec<Vec<Option<String>>>, CassieError> {
    let schema = cassie
        .catalog
        .get_schema(&statement.table)
        .ok_or_else(|| CassieError::CollectionNotFound(statement.table.clone()))?;
    let column_types = copy_column_types(statement, schema.fields.as_slice())?;
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
        return Err(CassieError::Unsupported(
            "COPY binary flags are not supported".to_string(),
        ));
    }
    let extension_length = i32::from_be_bytes(
        take(payload, &mut cursor, 4)?
            .try_into()
            .map_err(|_| CassieError::Parse("COPY binary header is truncated".to_string()))?,
    );
    let extension_length = usize::try_from(extension_length)
        .map_err(|_| CassieError::Parse("COPY binary extension length is negative".into()))?;
    let _extension = take(payload, &mut cursor, extension_length)?;

    let mut rows = Vec::new();
    loop {
        let field_count = i16::from_be_bytes(
            take(payload, &mut cursor, 2)?
                .try_into()
                .map_err(|_| CassieError::Parse("COPY binary row header is truncated".into()))?,
        );
        if field_count == -1 {
            if cursor != payload.len() {
                return Err(CassieError::Parse(
                    "COPY binary payload has trailing bytes after its trailer".into(),
                ));
            }
            return Ok(rows);
        }
        if field_count < 0 {
            return Err(CassieError::Parse(
                "COPY binary row has an invalid field count".to_string(),
            ));
        }
        let expected = i16::try_from(column_types.len())
            .map_err(|_| CassieError::ResourceLimit("COPY column count exceeds int16".into()))?;
        if field_count != expected {
            return Err(CassieError::Parse(format!(
                "COPY binary row has {field_count} columns but target expects {expected}"
            )));
        }
        let mut row = Vec::with_capacity(column_types.len());
        for (column_index, type_oid) in column_types.iter().copied().enumerate() {
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
            let value =
                super::super::codecs::binary_to_value(bytes, type_oid).map_err(|error| {
                    CassieError::Parse(format!(
                        "COPY binary field {} is invalid: {error}",
                        column_index + 1
                    ))
                })?;
            match value {
                Value::Null => row.push(None),
                value => row.push(Some(super::super::codecs::value_to_text(&value, type_oid))),
            }
        }
        rows.push(row);
    }
}

fn copy_column_types(
    statement: &CopyStatement,
    fields: &[crate::catalog::FieldMeta],
) -> Result<Vec<i64>, CassieError> {
    if statement.columns.is_empty() {
        return Ok(fields
            .iter()
            .map(|field| field.data_type.type_oid())
            .collect());
    }
    let declares_id =
        crate::types::row_identity::declares_id(fields.iter().map(|field| field.name.as_str()));
    statement
        .columns
        .iter()
        .map(|column| {
            if crate::types::row_identity::is_identity_reference(column, declares_id) {
                return Ok(DataType::Text.type_oid());
            }
            fields
                .iter()
                .find(|field| field.name == *column)
                .map(|field| field.data_type.type_oid())
                .ok_or_else(|| {
                    CassieError::Planner(format!(
                        "COPY target column '{column}' does not exist in '{}'",
                        statement.table
                    ))
                })
        })
        .collect()
}

fn decode_text_field(raw: &str, null_marker: &str) -> Option<String> {
    if raw == null_marker {
        None
    } else {
        Some(unescape_text_field(raw))
    }
}

fn unescape_text_field(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut characters = raw.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        let Some(escaped) = characters.next() else {
            output.push('\\');
            break;
        };
        output.push(match escaped {
            'b' => '\u{0008}',
            'f' => '\u{000c}',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'v' => '\u{000b}',
            other => other,
        });
    }
    output
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
