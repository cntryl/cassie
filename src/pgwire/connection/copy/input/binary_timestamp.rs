//! Fixed-stack TIMESTAMP output for admitted binary COPY fields.

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::extended::temporal::calendar::{
    timestamp_to_fields, TIMESTAMP_NOBEGIN, TIMESTAMP_NOEND,
};
use crate::types::{typed::TypedValue, Value};
use std::fmt::{self, Write as _};

#[cfg(test)]
#[path = "binary_timestamp_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "binary_timestamptz_tests.rs"]
mod timestamptz_tests;

struct Output {
    bytes: [u8; 64],
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

fn render(value: i64, utc: bool, output: &mut Output) -> fmt::Result {
    match value {
        TIMESTAMP_NOBEGIN => return output.write_str("-infinity"),
        TIMESTAMP_NOEND => return output.write_str("infinity"),
        _ => {}
    }
    let Some(fields) = timestamp_to_fields(value) else {
        return output.write_str("timestamp out of range");
    };
    let year = if fields.year > 0 {
        fields.year
    } else {
        -(fields.year - 1)
    };
    write!(
        output,
        "{year:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        fields.month, fields.day, fields.hour, fields.minute, fields.second
    )?;
    if fields.micros != 0 {
        write!(output, ".{:06}", fields.micros)?;
        while output.bytes[output.len - 1] == b'0' {
            output.len -= 1;
        }
    }
    if utc {
        output.write_str("+00")?;
    }
    if fields.year <= 0 {
        output.write_str(" BC")?;
    }
    Ok(())
}

pub(super) fn decode(
    oid: i64,
    bytes: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
) -> Result<Option<(String, QueryMemoryReservation)>, CassieError> {
    if !matches!(oid, 1114 | 1184) {
        return Ok(None);
    }
    controls.check_interruption()?;
    let value = super::super::super::codecs::binary_to_value(bytes, oid).map_err(|error| {
        super::super::super::codecs::timestamp_copy_error(&error).map_or_else(
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
    let (value, utc) = match value {
        Value::Typed(TypedValue::Timestamp(value)) => (value, false),
        Value::Typed(TypedValue::TimestampTz(value)) => (value, true),
        _ => {
            return Err(CassieError::Execution(
                "unexpected binary COPY timestamp value".into(),
            ));
        }
    };
    let mut output = Output {
        bytes: [0; 64],
        len: 0,
    };
    render(value, utc, &mut output).map_err(|_| {
        CassieError::Execution("binary COPY timestamp output exceeded stack".into())
    })?;
    let memory = controls.reserve_query_memory(output.len)?;
    let mut text = String::new();
    text.try_reserve_exact(output.len)
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if text.capacity() > output.len {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY timestamp capacity".into(),
        ));
    }
    let formatted = std::str::from_utf8(&output.bytes[..output.len])
        .map_err(|_| CassieError::Execution("invalid binary COPY timestamp output".into()))?;
    text.push_str(formatted);
    controls.check_interruption()?;
    Ok(Some((text, memory)))
}
