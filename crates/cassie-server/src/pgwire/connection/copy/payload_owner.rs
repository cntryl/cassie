//! Request-admitted ownership for the retained COPY input stream.

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[cfg(test)]
#[path = "payload_owner_tests.rs"]
mod tests;

pub(super) struct CopyPayload {
    bytes: Vec<u8>,
    memory: QueryMemoryReservation,
}

impl CopyPayload {
    pub(super) fn new(controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self {
            bytes: Vec::new(),
            memory: controls.reserve_query_memory(0)?,
        })
    }

    pub(super) fn len(&self) -> usize {
        self.bytes.len()
    }

    pub(super) fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    pub(super) fn append(
        &mut self,
        chunk: &[u8],
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        controls.check_interruption()?;
        let needed = self.bytes.len().checked_add(chunk.len()).ok_or_else(|| {
            CassieError::ResourceLimit("COPY input buffer capacity overflow".into())
        })?;
        if needed > self.bytes.capacity() {
            let capacity = needed.max(4).checked_next_power_of_two().ok_or_else(|| {
                CassieError::ResourceLimit("COPY input buffer capacity overflow".into())
            })?;
            let replacement_memory;
            let mut replacement;
            (replacement, replacement_memory) =
                crate::runtime::owned_memory::try_reserve_vec(capacity, &self.memory)?;
            for part in self.bytes.chunks(4096) {
                controls.check_interruption()?;
                replacement.extend_from_slice(part);
            }
            let old_bytes = self.bytes.capacity();
            let old = std::mem::replace(&mut self.bytes, replacement);
            drop(old);
            self.memory.shrink_to(self.memory.bytes() - old_bytes);
            self.memory.absorb(replacement_memory);
        }
        for part in chunk.chunks(4096) {
            controls.check_interruption()?;
            self.bytes.extend_from_slice(part);
        }
        Ok(())
    }
}

pub(super) fn copy_payload_exceeds_limit(
    current_bytes: usize,
    next_bytes: usize,
    limit: usize,
) -> bool {
    current_bytes.saturating_add(next_bytes) > limit
}

pub(super) fn copy_fail_message(
    message: &str,
    controls: &QueryExecutionControls,
) -> Result<(String, QueryMemoryReservation), CassieError> {
    let prefix = "COPY failed: ";
    let length = prefix
        .len()
        .checked_add(message.len())
        .ok_or_else(|| CassieError::ResourceLimit("COPY error capacity overflow".into()))?;
    let memory = controls.reserve_query_memory(length)?;
    let mut text = String::new();
    text.try_reserve_exact(length)
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if text.capacity() > length {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY error capacity".into(),
        ));
    }
    text.push_str(prefix);
    let mut start = 0;
    while start < message.len() {
        controls.check_interruption()?;
        let mut end = (start + 4096).min(message.len());
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        text.push_str(&message[start..end]);
        start = end;
    }
    Ok((text, memory))
}

pub(super) struct DecodedPayload {
    bytes: Vec<u8>,
    _memory: QueryMemoryReservation,
}

impl DecodedPayload {
    pub(super) fn as_slice(&self) -> &[u8] {
        &self.bytes
    }
}

pub(super) fn decode_latin1(
    payload: &[u8],
    controls: &QueryExecutionControls,
) -> Result<DecodedPayload, CassieError> {
    let mut length = 0_usize;
    for chunk in payload.chunks(4096) {
        controls.check_interruption()?;
        let extended = chunk.iter().filter(|byte| **byte >= 128).count();
        length = length
            .checked_add(chunk.len())
            .and_then(|size| size.checked_add(extended))
            .ok_or_else(|| CassieError::ResourceLimit("COPY encoding length overflow".into()))?;
    }
    let base = controls.reserve_query_memory(0)?;
    let memory;
    let mut bytes;
    (bytes, memory) = crate::runtime::owned_memory::try_reserve_vec(length, &base)?;
    for chunk in payload.chunks(4096) {
        controls.check_interruption()?;
        for byte in chunk {
            let mut encoded = [0_u8; 4];
            bytes.extend_from_slice(char::from(*byte).encode_utf8(&mut encoded).as_bytes());
        }
    }
    Ok(DecodedPayload {
        bytes,
        _memory: memory,
    })
}

pub(super) fn validate_utf8(
    payload: &[u8],
    controls: &QueryExecutionControls,
) -> Result<(), CassieError> {
    let mut start = 0;
    while start < payload.len() {
        controls.check_interruption()?;
        let mut end = (start + 4096).min(payload.len());
        while end > start && end < payload.len() && payload[end] & 0xc0 == 0x80 {
            end -= 1;
        }
        if end == start || std::str::from_utf8(&payload[start..end]).is_err() {
            return Err(crate::app::DiagnosticError::new(
                "22P04",
                "COPY payload is not valid UTF8",
            )
            .into());
        }
        start = end;
    }
    Ok(())
}

pub(super) fn decode_copy_payload(
    payload: &[u8],
    encoding: Option<&str>,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<Option<DecodedPayload>, CassieError> {
    let Some(encoding) = encoding else {
        return Ok(None);
    };
    match encoding.trim().to_ascii_uppercase().as_str() {
        "UTF8" | "UTF-8" | "UNICODE" => {
            validate_utf8(payload, controls)?;
            Ok(None)
        }
        "LATIN1" | "ISO-8859-1" | "ISO8859-1" => decode_latin1(payload, controls).map(Some),
        _ => Err(CassieError::Unsupported(format!(
            "COPY encoding '{encoding}' is not supported"
        ))),
    }
}
