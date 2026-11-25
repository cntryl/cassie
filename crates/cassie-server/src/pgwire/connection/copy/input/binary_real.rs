//! Fixed-stack REAL output for admitted binary COPY fields.

use std::fmt::{self, Write as _};

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::{typed::TypedValue, Value};

struct Output {
    bytes: [u8; 128],
    len: usize,
}

impl Output {
    const fn new() -> Self {
        Self {
            bytes: [0; 128],
            len: 0,
        }
    }

    fn text(&self) -> Result<&str, fmt::Error> {
        std::str::from_utf8(&self.bytes[..self.len]).map_err(|_| fmt::Error)
    }
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

fn format_real(value: f32, output: &mut Output) -> fmt::Result {
    if value.is_nan() {
        return output.write_str("NaN");
    }
    if value.is_infinite() {
        return output.write_str(if value.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        });
    }
    if value == 0.0 {
        return output.write_str(if value.is_sign_negative() { "-0" } else { "0" });
    }
    let mut scientific = Output::new();
    write!(&mut scientific, "{value:e}")?;
    let text = scientific.text()?;
    let Some((mantissa, exponent)) = text.split_once('e') else {
        return output.write_str(text);
    };
    let Ok(exponent) = exponent.parse::<i32>() else {
        return output.write_str(text);
    };
    if !(-4..6).contains(&exponent) {
        return write!(output, "{mantissa}e{exponent:+03}");
    }
    let (sign, mantissa) = mantissa
        .strip_prefix('-')
        .map_or(("", mantissa), |magnitude| ("-", magnitude));
    output.write_str(sign)?;
    let point = exponent + 1;
    if point <= 0 {
        output.write_str("0.")?;
        for _ in point..0 {
            output.write_char('0')?;
        }
    }
    let point = usize::try_from(point).unwrap_or_default();
    let mut digits = 0;
    for byte in mantissa.bytes().filter(|byte| *byte != b'.') {
        if point > 0 && digits == point {
            output.write_char('.')?;
        }
        output.write_char(char::from(byte))?;
        digits += 1;
    }
    for _ in digits..point {
        output.write_char('0')?;
    }
    Ok(())
}

pub(super) fn decode(
    oid: i64,
    bytes: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
) -> Result<Option<(String, QueryMemoryReservation)>, CassieError> {
    if oid != 700 {
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
    let Value::Typed(TypedValue::Real(value)) = value else {
        return Err(CassieError::Execution(
            "unexpected binary COPY real value".into(),
        ));
    };
    let mut output = Output::new();
    format_real(value, &mut output)
        .map_err(|_| CassieError::Execution("binary COPY real output exceeded stack".into()))?;
    let memory = controls.reserve_query_memory(output.len)?;
    let mut text = String::new();
    text.try_reserve_exact(output.len)
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if text.capacity() > output.len {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY real capacity".into(),
        ));
    }
    text.push_str(
        output
            .text()
            .map_err(|_| CassieError::Execution("invalid binary COPY real output".into()))?,
    );
    controls.check_interruption()?;
    Ok(Some((text, memory)))
}
