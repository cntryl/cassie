//! Fixed-stack TIMETZ output for admitted binary COPY fields.

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::{typed::TypedValue, Value};
use std::fmt::{self, Write as _};

#[cfg(test)]
#[path = "binary_timetz_tests.rs"]
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

fn render(time: i64, zone: i32, output: &mut Output) -> fmt::Result {
    let (hour, minute, second, micros) =
        crate::types::extended::temporal::calendar::time_fields(time);
    write!(output, "{hour:02}:{minute:02}:{second:02}")?;
    if micros != 0 {
        write!(output, ".{micros:06}")?;
        while output.bytes[output.len - 1] == b'0' {
            output.len -= 1;
        }
    }
    let east = -zone;
    let sign = if east < 0 { '-' } else { '+' };
    let magnitude = east.unsigned_abs();
    let (hours, minutes, seconds) = (magnitude / 3600, magnitude / 60 % 60, magnitude % 60);
    if seconds != 0 {
        write!(output, "{sign}{hours:02}:{minutes:02}:{seconds:02}")
    } else if minutes != 0 {
        write!(output, "{sign}{hours:02}:{minutes:02}")
    } else {
        write!(output, "{sign}{hours:02}")
    }
}

pub(super) fn decode(
    oid: i64,
    bytes: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
) -> Result<Option<(String, QueryMemoryReservation)>, CassieError> {
    if oid != 1266 {
        return Ok(None);
    }
    controls.check_interruption()?;
    let value = super::super::super::codecs::binary_to_value(bytes, oid).map_err(|error| {
        super::super::super::codecs::timetz_copy_error(&error).map_or_else(
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
    let Value::Typed(TypedValue::TimeTz(value)) = value else {
        return Err(CassieError::Execution(
            "unexpected binary COPY timetz value".into(),
        ));
    };
    let mut output = Output {
        bytes: [0; 32],
        len: 0,
    };
    render(value.time, value.zone, &mut output)
        .map_err(|_| CassieError::Execution("binary COPY timetz output exceeded stack".into()))?;
    let memory = controls.reserve_query_memory(output.len)?;
    let mut text = String::new();
    text.try_reserve_exact(output.len)
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if text.capacity() > output.len {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY timetz capacity".into(),
        ));
    }
    let formatted = std::str::from_utf8(&output.bytes[..output.len])
        .map_err(|_| CassieError::Execution("invalid binary COPY timetz output".into()))?;
    text.push_str(formatted);
    controls.check_interruption()?;
    Ok(Some((text, memory)))
}
