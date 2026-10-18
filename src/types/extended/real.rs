//! `real` (`float4`): input, output, ordering and binary framing.
use super::TypeInputError;

/// `float4in`.
///
/// # Errors
/// Returns `22P02` for malformed text and `22003` for overflow or underflow.
pub fn parse_text(text: &str) -> Result<f32, TypeInputError> {
    let trimmed = text.trim_matches(|c: char| c.is_ascii_whitespace() || c == '\u{b}');
    let magnitude = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    let negative = trimmed.starts_with('-');
    if magnitude.eq_ignore_ascii_case("nan") {
        return Ok(f32::NAN);
    }
    if magnitude.eq_ignore_ascii_case("inf") || magnitude.eq_ignore_ascii_case("infinity") {
        return Ok(if negative {
            f32::NEG_INFINITY
        } else {
            f32::INFINITY
        });
    }
    let parsed = trimmed.parse::<f32>().map_err(|_| TypeInputError {
        sqlstate: "22P02",
        message: format!("invalid input syntax for type real: \"{text}\""),
    })?;
    let nonzero = trimmed
        .split(['e', 'E'])
        .next()
        .is_some_and(|mantissa| mantissa.bytes().any(|digit| (b'1'..=b'9').contains(&digit)));
    if parsed.is_infinite() || (parsed == 0.0 && nonzero) {
        return Err(TypeInputError {
            sqlstate: "22003",
            message: format!("\"{text}\" is out of range for type real"),
        });
    }
    Ok(parsed)
}

/// `float84`/`i8tof4`: narrows a double, rejecting overflow and underflow.
///
/// # Errors
/// Returns `22003` when the value does not fit a `real`.
pub fn from_f64(value: f64, _source: &str) -> Result<f32, TypeInputError> {
    if value.is_nan() {
        return Ok(f32::NAN);
    }
    if value.is_infinite() {
        return Ok(if value.is_sign_negative() {
            f32::NEG_INFINITY
        } else {
            f32::INFINITY
        });
    }
    let narrowed = num_traits::ToPrimitive::to_f32(&value).unwrap_or(f32::INFINITY);
    if narrowed.is_infinite() {
        return Err(TypeInputError {
            sqlstate: "22003",
            message: "value out of range: overflow".to_string(),
        });
    }
    if narrowed == 0.0 && value != 0.0 {
        return Err(TypeInputError {
            sqlstate: "22003",
            message: "value out of range: underflow".to_string(),
        });
    }
    Ok(narrowed)
}

/// `float4out`: the shortest round-trip digits, in exponent form below
/// `1e-4` or from `1e6` (`FLT_DIG`) upward.
#[must_use]
pub fn format_text(value: f32) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .to_string();
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.to_string();
    }
    let scientific = format!("{value:e}");
    let Some((mantissa, exponent)) = scientific.split_once('e') else {
        return scientific;
    };
    let Ok(exponent) = exponent.parse::<i32>() else {
        return scientific;
    };
    if !(-4..6).contains(&exponent) {
        return format!("{mantissa}e{exponent:+03}");
    }
    let (sign, digits) = mantissa
        .strip_prefix('-')
        .map_or(("", mantissa), |magnitude| ("-", magnitude));
    let digits = digits.replace('.', "");
    let point = exponent + 1;
    let mut output = String::from(sign);
    if point <= 0 {
        output.push_str("0.");
        for _ in point..0 {
            output.push('0');
        }
        output.push_str(&digits);
    } else {
        let point = usize::try_from(point).unwrap_or_default();
        if point < digits.len() {
            output.push_str(&digits[..point]);
            output.push('.');
            output.push_str(&digits[point..]);
        } else {
            output.push_str(&digits);
            for _ in digits.len()..point {
                output.push('0');
            }
        }
    }
    output
}

/// A key whose unsigned order is PostgreSQL's `float4` order: negative
/// values, then zero (both signs), then positive values, then NaN.
#[must_use]
pub fn order_key(value: f32) -> u32 {
    if value.is_nan() {
        return u32::MAX;
    }
    let value = if value == 0.0 { 0.0 } else { value };
    let bits = value.to_bits();
    if value.is_sign_negative() {
        !bits
    } else {
        bits | 0x8000_0000
    }
}

/// `float4recv`.
///
/// # Errors
/// Returns a message when the frame is not four bytes.
pub fn receive_binary(bytes: &[u8]) -> Result<f32, String> {
    <[u8; 4]>::try_from(bytes)
        .map(f32::from_be_bytes)
        .map_err(|_| "invalid binary real frame".to_string())
}

#[cfg(test)]
mod tests {
    use super::{format_text, from_f64, order_key, parse_text};

    #[test]
    fn should_format_float4_like_postgresql() {
        // Arrange
        let values = [1e5_f32, 1e6, 123_456_789.0, 0.0001, 0.000_01, 0.1, -0.0];

        // Act
        let formatted = values.map(format_text);

        // Assert
        assert_eq!(
            formatted,
            [
                "100000",
                "1e+06",
                "1.2345679e+08",
                "0.0001",
                "1e-05",
                "0.1",
                "-0"
            ]
        );
    }

    #[test]
    fn should_reject_float4_overflow_and_syntax_with_sqlstates() {
        // Arrange
        let inputs = ["1e39", "abc", "1e-46"];

        // Act
        let states = inputs.map(|input| parse_text(input).map_err(|error| error.sqlstate));

        // Assert
        assert_eq!(states, [Err("22003"), Err("22P02"), Err("22003")]);
    }

    #[test]
    fn should_reject_narrowing_overflow_and_underflow() {
        // Arrange
        let values = [1e39_f64, 1e-46];

        // Act
        let narrowed = values.map(|value| from_f64(value, "").map_err(|error| error.message));

        // Assert
        assert_eq!(
            narrowed,
            [
                Err("value out of range: overflow".to_string()),
                Err("value out of range: underflow".to_string())
            ]
        );
    }

    #[test]
    fn should_order_float4_with_nan_last_and_equal_zeros() {
        // Arrange
        let values = [
            f32::NAN,
            f32::INFINITY,
            1.0,
            0.0,
            -0.0,
            -1.0,
            f32::NEG_INFINITY,
        ];

        // Act
        let mut keys = values.map(order_key).to_vec();
        keys.sort_unstable();

        // Assert
        assert_eq!(order_key(0.0), order_key(-0.0));
        assert_eq!(keys.last(), Some(&order_key(f32::NAN)));
        assert_eq!(keys.first(), Some(&order_key(f32::NEG_INFINITY)));
    }
}
