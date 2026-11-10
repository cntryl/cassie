//! Capacity-coupled CSV COPY decoding for wire and embedded commands.

use super::{CassieError, CopyCsvOptions};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

type ParsedCsvRows = Vec<Vec<Option<String>>>;
type ParsedCsvPayload = (ParsedCsvRows, QueryMemoryReservation);

#[cfg(test)]
#[path = "copy_csv_tests.rs"]
mod tests;

fn next_char(bytes: &[u8], offset: usize) -> Result<(char, usize), CassieError> {
    let first = bytes[offset];
    let length = if first < 0x80 {
        1
    } else if first < 0xe0 {
        2
    } else if first < 0xf0 {
        3
    } else {
        4
    };
    let end = offset
        .checked_add(length)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| CassieError::Parse("COPY payload must be UTF-8".into()))?;
    let text = std::str::from_utf8(&bytes[offset..end])
        .map_err(|_| CassieError::Parse("COPY payload must be UTF-8".into()))?;
    Ok((text.chars().next().expect("nonempty UTF-8 character"), end))
}

pub(crate) fn parse_csv_payload(
    payload: &[u8],
    options: CopyCsvOptions<'_>,
    empty_unquoted_is_null: bool,
    controls: &QueryExecutionControls,
) -> Result<ParsedCsvPayload, CassieError> {
    super::copy_decode::validate_payload_utf8(payload, controls)?;
    let mut memory = controls.reserve_query_memory(0)?;
    let mut row_memory = controls.reserve_query_memory(0)?;
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut start = 0;
    let mut index = 0;
    let mut next_check = 0;
    let mut in_quotes = false;
    let mut quoted = false;
    let mut started = false;
    while index < payload.len() {
        if index >= next_check {
            controls.check_interruption()?;
            next_check = index.saturating_add(4096);
        }
        let (character, next) = next_char(payload, index)?;
        let escaped_quote = in_quotes
            && character == options.escape
            && next < payload.len()
            && next_char(payload, next)?.0 == options.quote;
        if escaped_quote {
            index = next_char(payload, next)?.1;
            continue;
        }
        if character == options.quote {
            in_quotes = !in_quotes;
            quoted = true;
            started = true;
            index = next;
            continue;
        }
        let delimiter = character == options.delimiter && !in_quotes;
        let newline = (character == '\n' || character == '\r') && !in_quotes;
        if delimiter || newline {
            push_field(
                &mut row,
                &mut row_memory,
                &payload[start..index],
                quoted,
                options,
                empty_unquoted_is_null,
                controls,
            )?;
            index = next;
            if newline && character == '\r' && payload.get(index) == Some(&b'\n') {
                index += 1;
            }
            start = index;
            quoted = false;
            started = false;
            if newline {
                super::copy_decode::grow_text_vec(&mut rows, &mut memory, controls)?;
                rows.push(std::mem::take(&mut row));
                memory.absorb(row_memory);
                row_memory = controls.reserve_query_memory(0)?;
            }
        } else {
            started = true;
            index = next;
        }
    }
    if in_quotes {
        return Err(crate::app::DiagnosticError::new(
            "22P04",
            "COPY CSV payload has unterminated quote",
        )
        .into());
    }
    if started || !row.is_empty() {
        push_field(
            &mut row,
            &mut row_memory,
            &payload[start..],
            quoted,
            options,
            empty_unquoted_is_null,
            controls,
        )?;
        super::copy_decode::grow_text_vec(&mut rows, &mut memory, controls)?;
        rows.push(row);
        memory.absorb(row_memory);
    }
    Ok((rows, memory))
}

fn push_field(
    row: &mut Vec<Option<String>>,
    memory: &mut QueryMemoryReservation,
    raw: &[u8],
    quoted: bool,
    options: CopyCsvOptions<'_>,
    empty_unquoted_is_null: bool,
    controls: &QueryExecutionControls,
) -> Result<(), CassieError> {
    super::copy_decode::grow_text_vec(row, memory, controls)?;
    let field_memory = controls.reserve_query_memory(raw.len())?;
    let mut field = String::new();
    field
        .try_reserve_exact(raw.len())
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if field.capacity() > raw.len() {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY CSV field capacity".into(),
        ));
    }
    let mut index = 0;
    let mut next_check = 0;
    let mut in_quotes = false;
    while index < raw.len() {
        if index >= next_check {
            controls.check_interruption()?;
            next_check = index.saturating_add(4096);
        }
        let (character, next) = next_char(raw, index)?;
        if in_quotes
            && character == options.escape
            && next < raw.len()
            && next_char(raw, next)?.0 == options.quote
        {
            field.push(options.quote);
            index = next_char(raw, next)?.1;
        } else if character == options.quote {
            in_quotes = !in_quotes;
            index = next;
        } else {
            field.push(character);
            index = next;
        }
    }
    let mut marker_match = field.len() == options.null_marker.len();
    if marker_match {
        for (left, right) in field
            .as_bytes()
            .chunks(4096)
            .zip(options.null_marker.as_bytes().chunks(4096))
        {
            controls.check_interruption()?;
            if left != right {
                marker_match = false;
                break;
            }
        }
    }
    let empty_match = empty_unquoted_is_null && field.is_empty() && !quoted;
    let null = if quoted {
        marker_match && options.force_null.contains(&row.len())
    } else {
        (marker_match || empty_match) && !options.force_not_null.contains(&row.len())
    };
    if null {
        drop(field);
        drop(field_memory);
        row.push(None);
    } else {
        memory.absorb(field_memory);
        row.push(Some(field));
    }
    Ok(())
}
