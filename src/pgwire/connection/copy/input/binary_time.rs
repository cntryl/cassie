//! Fixed-stack TIME output for admitted binary COPY fields.

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::{typed::TypedValue, Value};
use std::fmt::{self, Write as _};

#[cfg(test)]
#[path = "binary_time_tests.rs"]
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
    if oid != 1083 {
        return Ok(None);
    }
    controls.check_interruption()?;
    let value = super::super::super::codecs::binary_to_value(bytes, oid).map_err(|error| {
        super::super::super::codecs::temporal_copy_error(&error).map_or_else(
            || {
                CassieError::Parse(format!(
                    "COPY binary field {} is invalid: {error}",
                    column + 1
                ))
            },
            CassieError::TypeInput,
        )
    })?;
    controls.check_interruption()?;
    let Value::Typed(TypedValue::Time(value)) = value else {
        return Err(CassieError::Execution(
            "unexpected binary COPY time value".into(),
        ));
    };
    let (hour, minute, second, micros) =
        crate::types::extended::temporal::calendar::time_fields(value);
    let mut output = Output {
        bytes: [0; 32],
        len: 0,
    };
    write!(&mut output, "{hour:02}:{minute:02}:{second:02}")
        .map_err(|_| CassieError::Execution("binary COPY time output exceeded stack".into()))?;
    if micros != 0 {
        write!(&mut output, ".{micros:06}")
            .map_err(|_| CassieError::Execution("binary COPY time output exceeded stack".into()))?;
        while output.bytes[output.len - 1] == b'0' {
            output.len -= 1;
        }
    }
    let memory = controls.reserve_query_memory(output.len)?;
    let mut text = String::new();
    text.try_reserve_exact(output.len)
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if text.capacity() > output.len {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY time capacity".into(),
        ));
    }
    let formatted = std::str::from_utf8(&output.bytes[..output.len])
        .map_err(|_| CassieError::Execution("invalid binary COPY time output".into()))?;
    text.push_str(formatted);
    controls.check_interruption()?;
    Ok(Some((text, memory)))
}
