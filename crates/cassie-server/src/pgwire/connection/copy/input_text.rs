//! COPY text-format payload decoding: record splitting, the `\.` end-of-data
//! marker, and backslash escape decoding.

use crate::app::{CassieError, DiagnosticError};

type CopyRowsWithMemory = (
    Vec<Vec<Option<String>>>,
    crate::runtime::QueryMemoryReservation,
);

#[derive(Clone, Copy, PartialEq, Eq)]
enum LineEnding {
    Lf,
    Crlf,
}

fn not_alone() -> CassieError {
    DiagnosticError::new("22P04", "end-of-copy marker is not alone on its line").into()
}

/// Classifies a `\.` found at `index`: `Ok(())` when it is a valid end-of-data
/// marker, otherwise the PostgreSQL format error.
fn check_end_marker(
    payload: &[u8],
    index: usize,
    line_start: usize,
    ending: Option<LineEnding>,
) -> Result<(), CassieError> {
    if index != line_start {
        return Err(not_alone());
    }
    let rest = &payload[index + 2..];
    let style = if rest.starts_with(b"\n") {
        LineEnding::Lf
    } else if rest.starts_with(b"\r\n") {
        LineEnding::Crlf
    } else {
        return Err(not_alone());
    };
    if ending.is_some_and(|previous| previous != style) {
        return Err(DiagnosticError::new(
            "22P04",
            "end-of-copy marker does not match previous newline style",
        )
        .into());
    }
    Ok(())
}

pub(super) fn parse_text_payload(
    payload: &[u8],
    delimiter: char,
    null_marker: &str,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<CopyRowsWithMemory, CassieError> {
    let mut memory = controls.reserve_query_memory(0)?;
    let mut row_memory = controls.reserve_query_memory(0)?;
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut delimiter_bytes = [0_u8; 4];
    let delimiter = delimiter.encode_utf8(&mut delimiter_bytes).as_bytes();
    let mut field_start = 0;
    let mut line_start = 0;
    let mut ending = None;
    let mut terminated = false;
    let mut index = 0;
    let mut next_check = 0;
    while index < payload.len() {
        if index >= next_check {
            controls.check_interruption()?;
            next_check = index.saturating_add(4096);
        }
        if payload[index] == b'\\' {
            if payload.get(index + 1) == Some(&b'.') {
                check_end_marker(payload, index, line_start, ending)?;
                terminated = true;
                break;
            }
            // An escaped byte never splits a field or a row.
            index = (index + 2).min(payload.len());
            continue;
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
                line_start = index;
                let style = if crlf {
                    LineEnding::Crlf
                } else {
                    LineEnding::Lf
                };
                if ending.is_some_and(|previous| previous != style) {
                    return Err(DiagnosticError::new(
                        "22P04",
                        "mixed record endings in text COPY data",
                    )
                    .into());
                }
                ending = Some(style);
                crate::app::copy_decode::grow_text_vec(&mut rows, &mut memory, controls)?;
                rows.push(std::mem::take(&mut row));
                memory.absorb(row_memory);
                row_memory = controls.reserve_query_memory(0)?;
            }
        } else {
            index += 1;
        }
    }
    if !terminated && (field_start < payload.len() || !row.is_empty()) {
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
    // Only consumed fields need encoding validation. Check the raw field
    // before unescaping so an escaped byte cannot repair invalid input.
    crate::app::copy_decode::validate_payload_utf8(raw, controls).map_err(|error| match error {
        CassieError::Parse(_) => invalid_utf8(),
        other => other,
    })?;
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
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(raw.len())
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if bytes.capacity() > raw.len() {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY field capacity".into(),
        ));
    }
    decode_escapes(raw, &mut bytes, controls)?;
    let field = String::from_utf8(bytes).map_err(|_| invalid_utf8())?;
    memory.absorb(field_memory);
    row.push(Some(field));
    Ok(())
}

fn invalid_utf8() -> CassieError {
    DiagnosticError::new("22021", "COPY payload must be UTF-8").into()
}

fn hex_value(byte: Option<&u8>) -> Option<u32> {
    byte.and_then(|byte| char::from(*byte).to_digit(16))
}

fn nul_error() -> CassieError {
    DiagnosticError::new("22021", "invalid byte sequence for encoding \"UTF8\": 0x00").into()
}

/// Decodes backslash escapes the way PostgreSQL's text COPY does: the control
/// escapes, one to three octal digits, one or two hex digits, and a backslash
/// before any other byte standing for that byte. A lone backslash at the very
/// end of the field is dropped.
fn decode_escapes(
    raw: &[u8],
    out: &mut Vec<u8>,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<(), CassieError> {
    let mut index = 0;
    let mut next_check = 0;
    while index < raw.len() {
        if index >= next_check {
            controls.check_interruption()?;
            next_check = index.saturating_add(4096);
        }
        let byte = raw[index];
        index += 1;
        if byte != b'\\' {
            out.push(byte);
            continue;
        }
        let Some(&escaped) = raw.get(index) else {
            break;
        };
        index += 1;
        let decoded = match escaped {
            b'b' => 0x08,
            b'f' => 0x0c,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => 0x0b,
            b'0'..=b'7' => {
                let mut value = u32::from(escaped - b'0');
                for _ in 0..2 {
                    match raw.get(index) {
                        Some(digit @ b'0'..=b'7') => {
                            value = value * 8 + u32::from(digit - b'0');
                            index += 1;
                        }
                        _ => break,
                    }
                }
                u8::try_from(value & 0xff).unwrap_or_default()
            }
            b'x' => {
                if let Some(high) = hex_value(raw.get(index)) {
                    index += 1;
                    let mut value = high;
                    if let Some(low) = hex_value(raw.get(index)) {
                        value = value * 16 + low;
                        index += 1;
                    }
                    u8::try_from(value).unwrap_or_default()
                } else {
                    b'x'
                }
            }
            other => other,
        };
        if decoded == 0 {
            return Err(nul_error());
        }
        out.push(decoded);
    }
    Ok(())
}
