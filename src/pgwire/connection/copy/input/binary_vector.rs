//! Two-pass admitted VECTOR output for binary COPY fields.
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use std::fmt::{self, Write as _};

#[cfg(test)]
#[path = "binary_vector_tests.rs"]
mod tests;

struct Component {
    bytes: [u8; 128],
    len: usize,
}

impl fmt::Write for Component {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(fmt::Error)?;
        self.bytes
            .get_mut(self.len..end)
            .ok_or(fmt::Error)?
            .copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

fn component(bytes: &[u8], index: usize) -> f32 {
    let offset = 4 + index * 4;
    f32::from_be_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn format_component(value: f32) -> Result<Component, CassieError> {
    let mut output = Component {
        bytes: [0; 128],
        len: 0,
    };
    write!(&mut output, "{value}").map_err(|_| {
        CassieError::Execution("binary COPY vector component exceeded stack".into())
    })?;
    Ok(output)
}

fn invalid(column: usize) -> CassieError {
    CassieError::Parse(format!(
        "COPY binary field {} is invalid: invalid binary vector value",
        column + 1
    ))
}

pub(super) fn decode(
    bytes: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
    expected_dimensions: Option<usize>,
) -> Result<(String, QueryMemoryReservation), CassieError> {
    decode_with_observer(bytes, column, controls, expected_dimensions, |_| {})
}

#[cfg(test)]
fn decode_observed(
    bytes: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
    expected_dimensions: Option<usize>,
    observer: impl FnMut(bool),
) -> Result<(String, QueryMemoryReservation), CassieError> {
    decode_with_observer(bytes, column, controls, expected_dimensions, observer)
}

fn decode_with_observer(
    bytes: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
    expected_dimensions: Option<usize>,
    mut observer: impl FnMut(bool),
) -> Result<(String, QueryMemoryReservation), CassieError> {
    controls.check_interruption()?;
    if bytes.len() < 4 {
        return Err(invalid(column));
    }
    let dimensions = usize::from(u16::from_be_bytes([bytes[0], bytes[1]]));
    let reserved = i16::from_be_bytes([bytes[2], bytes[3]]);
    if !(1..=16_000).contains(&dimensions)
        || expected_dimensions.is_some_and(|expected| dimensions != expected)
        || reserved != 0
        || bytes.len() != 4 + dimensions * 4
    {
        return Err(invalid(column));
    }
    let mut length = 2_usize
        .checked_add(dimensions - 1)
        .ok_or_else(|| CassieError::ResourceLimit("COPY vector output length overflow".into()))?;
    for index in 0..dimensions {
        if index.is_multiple_of(128) {
            if index != 0 {
                observer(false);
            }
            controls.check_interruption()?;
        }
        let value = component(bytes, index);
        if !value.is_finite() {
            return Err(invalid(column));
        }
        let formatted = format_component(value)?;
        length = length.checked_add(formatted.len).ok_or_else(|| {
            CassieError::ResourceLimit("COPY vector output length overflow".into())
        })?;
    }
    controls.check_interruption()?;
    let memory = controls.reserve_query_memory(length)?;
    let mut text = String::new();
    text.try_reserve_exact(length)
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if text.capacity() > length {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY vector capacity".into(),
        ));
    }
    text.push('[');
    for index in 0..dimensions {
        if index.is_multiple_of(128) {
            observer(true);
            controls.check_interruption()?;
        }
        if index != 0 {
            text.push(',');
        }
        let formatted = format_component(component(bytes, index))?;
        let value = std::str::from_utf8(&formatted.bytes[..formatted.len])
            .map_err(|_| CassieError::Execution("invalid binary COPY vector output".into()))?;
        text.push_str(value);
    }
    text.push(']');
    controls.check_interruption()?;
    Ok((text, memory))
}
