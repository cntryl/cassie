//! PostgreSQL's default FLOAT8 spelling, including strict rounding boundaries.
use std::fmt::{self, Write as _};

#[path = "float_text/dyadic.rs"]
mod dyadic;

#[cfg(test)]
#[path = "float_text/tests.rs"]
mod tests;

pub(crate) struct StackText {
    bytes: [u8; 128],
    len: usize,
}

impl StackText {
    pub(crate) const fn new() -> Self {
        Self {
            bytes: [0; 128],
            len: 0,
        }
    }

    pub(crate) fn as_str(&self) -> Result<&str, fmt::Error> {
        std::str::from_utf8(&self.bytes[..self.len]).map_err(|_| fmt::Error)
    }
}

impl fmt::Write for StackText {
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

pub(crate) fn format(value: f64) -> String {
    let mut output = if value.is_finite() && value != 0.0 {
        String::with_capacity(25)
    } else {
        String::new()
    };
    format_to(value, &mut output).expect("String float formatting");
    output
}

pub(crate) fn format_to(value: f64, output: &mut impl fmt::Write) -> fmt::Result {
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
    let decimal = shortest_decimal(value)?;
    let (mantissa, exponent) = decimal
        .as_str()?
        .split_once('e')
        .expect("Rust scientific notation");
    let exponent = exponent.parse::<i32>().expect("Rust decimal exponent");
    let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
    if !(-4..15).contains(&exponent) {
        return write!(output, "{mantissa}e{exponent:+03}");
    }
    let digits = if let Some(magnitude) = mantissa.strip_prefix('-') {
        output.write_char('-')?;
        magnitude
    } else {
        mantissa
    };
    let point = exponent + 1;
    if point <= 0 {
        output.write_str("0.")?;
        for _ in point..0 {
            output.write_char('0')?;
        }
    }
    let point = usize::try_from(point).unwrap_or_default();
    let mut count = 0;
    for byte in digits.bytes().filter(|byte| *byte != b'.') {
        if point > 0 && count == point {
            output.write_char('.')?;
        }
        output.write_char(char::from(byte))?;
        count += 1;
    }
    for _ in count..point {
        output.write_char('0')?;
    }
    Ok(())
}

fn shortest_decimal(value: f64) -> Result<StackText, fmt::Error> {
    let mut shortest = StackText::new();
    write!(&mut shortest, "{value:e}")?;
    let (mantissa, _) = shortest
        .as_str()?
        .split_once('e')
        .expect("Rust scientific notation");
    let precision = mantissa.bytes().filter(u8::is_ascii_digit).count() - 1;
    // Explicit precision rounds ties to even; PostgreSQL rejects binary midpoints.
    for precision in precision..=17 {
        let mut candidate = StackText::new();
        write!(&mut candidate, "{value:.precision$e}")?;
        let text = candidate.as_str()?;
        if text
            .parse::<f64>()
            .is_ok_and(|parsed| parsed.to_bits() == value.to_bits())
            && !dyadic::is_midpoint(value, text)
        {
            return Ok(candidate);
        }
        let text = shortest.as_str()?;
        if text
            .parse::<f64>()
            .is_ok_and(|parsed| parsed.to_bits() == value.to_bits())
            && !dyadic::is_midpoint(value, text)
        {
            return Ok(shortest);
        }
    }
    unreachable!("18 significant digits strictly roundtrip a finite double")
}
