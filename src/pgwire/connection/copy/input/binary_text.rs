//! Direct admitted binary conversion for text-family and bytea fields.

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

fn invalid_text(bytes: &[u8], oid: i64, column: usize) -> CassieError {
    let error = super::super::super::codecs::binary_to_value(bytes, oid)
        .expect_err("invalid UTF-8 must be rejected by the text codec");
    CassieError::Parse(format!(
        "COPY binary field {} is invalid: {error}",
        column + 1
    ))
}

pub(super) fn decode(
    oid: i64,
    bytes: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
) -> Result<Option<(String, QueryMemoryReservation)>, CassieError> {
    decode_observed(oid, bytes, column, controls, || {})
}

pub(super) fn decode_observed(
    oid: i64,
    bytes: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
    mut after_chunk: impl FnMut(),
) -> Result<Option<(String, QueryMemoryReservation)>, CassieError> {
    if !matches!(oid, 17 | 25 | 1042 | 1043) {
        return Ok(None);
    }
    controls.check_interruption()?;
    if oid != 17 {
        let mut start = 0;
        while start < bytes.len() {
            controls.check_interruption()?;
            let mut end = (start + 4096).min(bytes.len());
            while end > start && end < bytes.len() && bytes[end] & 0xc0 == 0x80 {
                end -= 1;
            }
            if end == start {
                return Err(invalid_text(&bytes[start..start + 1], oid, column));
            }
            if std::str::from_utf8(&bytes[start..end]).is_err() {
                return Err(invalid_text(&bytes[start..end], oid, column));
            }
            start = end;
            after_chunk();
        }
        controls.check_interruption()?;
    }
    let length = if oid == 17 {
        bytes
            .len()
            .checked_mul(2)
            .and_then(|value| value.checked_add(2))
            .ok_or_else(|| {
                CassieError::ResourceLimit("COPY binary bytea capacity overflow".into())
            })?
    } else {
        bytes.len()
    };
    let memory = controls.reserve_query_memory(length)?;
    let mut output = String::new();
    output
        .try_reserve_exact(length)
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if output.capacity() > length {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY binary text capacity".into(),
        ));
    }
    if oid == 17 {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        output.push_str("\\x");
        for chunk in bytes.chunks(4096) {
            controls.check_interruption()?;
            for byte in chunk {
                output.push(char::from(HEX[usize::from(byte >> 4)]));
                output.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
            after_chunk();
        }
    } else {
        let mut start = 0;
        while start < bytes.len() {
            controls.check_interruption()?;
            let mut end = (start + 4096).min(bytes.len());
            while end > start && end < bytes.len() && bytes[end] & 0xc0 == 0x80 {
                end -= 1;
            }
            let chunk = std::str::from_utf8(&bytes[start..end])
                .map_err(|_| invalid_text(&bytes[start..end], oid, column))?;
            output.push_str(chunk);
            start = end;
            after_chunk();
        }
    }
    controls.check_interruption()?;
    Ok(Some((output, memory)))
}
