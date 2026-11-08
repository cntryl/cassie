//! Fixed-stack DATE output for admitted binary COPY fields.

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::extended::temporal::calendar::{date_to_fields, DATE_NOBEGIN, DATE_NOEND};
use crate::types::{typed::TypedValue, Value};
use std::fmt::{self, Write as _};

#[cfg(test)]
#[path = "binary_date_tests.rs"]
mod tests;

struct Output {
    bytes: [u8; 32],
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
    if oid != 1082 {
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
    let Value::Typed(TypedValue::Date(value)) = value else {
        return Err(CassieError::Execution(
            "unexpected binary COPY date value".into(),
        ));
    };
    let mut output = Output {
        bytes: [0; 32],
        len: 0,
    };
    let formatted = match value {
        DATE_NOBEGIN => output.write_str("-infinity"),
        DATE_NOEND => output.write_str("infinity"),
        _ => {
            let (year, month, day) = date_to_fields(value);
            let shown = if year > 0 { year } else { -(year - 1) };
            write!(&mut output, "{shown:04}-{month:02}-{day:02}").and_then(|()| {
                if year <= 0 {
                    output.write_str(" BC")
                } else {
                    Ok(())
                }
            })
        }
    };
    formatted
        .map_err(|_| CassieError::Execution("binary COPY date output exceeded stack".into()))?;
    let memory = controls.reserve_query_memory(output.len)?;
    let mut text = String::new();
    text.try_reserve_exact(output.len)
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if text.capacity() > output.len {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY date capacity".into(),
        ));
    }
    let formatted = std::str::from_utf8(&output.bytes[..output.len])
        .map_err(|_| CassieError::Execution("invalid binary COPY date output".into()))?;
    text.push_str(formatted);
    controls.check_interruption()?;
    Ok(Some((text, memory)))
}
