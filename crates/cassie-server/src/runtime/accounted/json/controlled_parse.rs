//! Partial checked-reader JSON Value ownership for binary COPY.
use std::io::{self, Read};

use crate::app::CassieError;
use crate::runtime::{accounted::Accounted, QueryExecutionControls};

#[cfg(test)]
#[path = "controlled_parse_tests.rs"]
mod tests;

#[derive(Debug)]
pub(crate) enum ParseError {
    InvalidUtf8,
    InvalidJson {
        error: serde_json::Error,
        _memory: crate::runtime::QueryMemoryReservation,
    },
    Controlled(CassieError),
}

struct CheckedReader<'a, F> {
    remaining: &'a [u8],
    consumed: usize,
    next_check: usize,
    controls: &'a QueryExecutionControls,
    interruption: &'a mut Option<CassieError>,
    observer: F,
}

impl<F: FnMut()> Read for CheckedReader<'_, F> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.consumed >= self.next_check {
            if let Err(error) = self.controls.check_interruption() {
                *self.interruption = Some(error);
                return Err(io::Error::other("query interrupted"));
            }
            (self.observer)();
            if let Err(error) = self.controls.check_interruption() {
                *self.interruption = Some(error);
                return Err(io::Error::other("query interrupted"));
            }
            self.next_check = self.consumed.saturating_add(1024);
        }
        let count = self
            .remaining
            .len()
            .min(buffer.len())
            .min(self.next_check - self.consumed);
        buffer[..count].copy_from_slice(&self.remaining[..count]);
        self.remaining = &self.remaining[count..];
        self.consumed += count;
        Ok(count)
    }
}

fn checked_utf8(bytes: &[u8], controls: &QueryExecutionControls) -> Result<(), ParseError> {
    let mut start = 0;
    while start < bytes.len() {
        controls
            .check_interruption()
            .map_err(ParseError::Controlled)?;
        let mut end = start.saturating_add(4096).min(bytes.len());
        while end > start && end < bytes.len() && bytes[end] & 0xc0 == 0x80 {
            end -= 1;
        }
        if end == start || std::str::from_utf8(&bytes[start..end]).is_err() {
            return Err(ParseError::InvalidUtf8);
        }
        start = end;
    }
    controls
        .check_interruption()
        .map_err(ParseError::Controlled)
}

pub(crate) fn parse(
    bytes: &[u8],
    controls: &QueryExecutionControls,
) -> Result<Accounted<serde_json::Value>, ParseError> {
    parse_with_observer(bytes, controls, || {})
}

#[cfg(test)]
fn parse_observed(
    bytes: &[u8],
    controls: &QueryExecutionControls,
    observer: impl FnMut(),
) -> Result<Accounted<serde_json::Value>, ParseError> {
    parse_with_observer(bytes, controls, observer)
}

fn parse_with_observer(
    bytes: &[u8],
    controls: &QueryExecutionControls,
    observer: impl FnMut(),
) -> Result<Accounted<serde_json::Value>, ParseError> {
    checked_utf8(bytes, controls)?;
    let bound =
        super::serialized_decode_bytes(bytes.len().max(1)).map_err(ParseError::Controlled)?;
    let memory = controls
        .reserve_query_memory(bound)
        .map_err(ParseError::Controlled)?;
    controls
        .check_interruption()
        .map_err(ParseError::Controlled)?;
    let mut interruption = None;
    let reader = CheckedReader {
        remaining: bytes,
        consumed: 0,
        next_check: 1024,
        controls,
        interruption: &mut interruption,
        observer,
    };
    let result = serde_json::from_reader(reader);
    if let Some(error) = interruption {
        return Err(ParseError::Controlled(error));
    }
    let value = match result {
        Ok(value) => value,
        Err(error) => {
            return Err(ParseError::InvalidJson {
                error,
                _memory: memory,
            })
        }
    };
    controls
        .check_interruption()
        .map_err(ParseError::Controlled)?;
    Ok(Accounted::from_admitted(value, memory))
}
