//! Fixed-stack INTERVAL output for admitted binary COPY fields.

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::{typed::TypedValue, Value};
use std::fmt;

#[cfg(test)]
#[path = "binary_interval_tests.rs"]
mod tests;

struct Output {
    bytes: [u8; 128],
    len: usize,
}

impl fmt::Write for Output {
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

pub(super) fn decode(
    oid: i64,
    bytes: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
) -> Result<Option<(String, QueryMemoryReservation)>, CassieError> {
    if oid != 1186 {
        return Ok(None);
    }
    controls.check_interruption()?;
    let value = super::super::super::codecs::binary_to_value(bytes, oid).map_err(|error| {
        CassieError::Parse(format!(
            "COPY binary field {} is invalid: {error}",
            column + 1
        ))
    })?;
    controls.check_interruption()?;
    let Value::Typed(TypedValue::Interval(value)) = value else {
        return Err(CassieError::Execution(
            "unexpected binary COPY interval value".into(),
        ));
    };
    let mut output = Output {
        bytes: [0; 128],
        len: 0,
    };
    value
        .format_postgres_to(&mut output)
        .map_err(|_| CassieError::Execution("binary COPY interval output exceeded stack".into()))?;
    let memory = controls.reserve_query_memory(output.len)?;
    let mut text = String::new();
    text.try_reserve_exact(output.len)
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if text.capacity() > output.len {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY interval capacity".into(),
        ));
    }
    let formatted = std::str::from_utf8(&output.bytes[..output.len])
        .map_err(|_| CassieError::Execution("invalid binary COPY interval output".into()))?;
    text.push_str(formatted);
    controls.check_interruption()?;
    Ok(Some((text, memory)))
}
