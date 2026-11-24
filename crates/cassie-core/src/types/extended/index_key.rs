//! Order-preserving, value-canonical index keys for `numeric` and `real`.
//!
//! Scalar-index and unique-reservation keys are built from JSON carriers, and a
//! `numeric` carrier may be the text `"1.0"` or the integer `1` for equal
//! values, with unrelated tags and text order. These helpers rewrite a carrier
//! to a printable string whose byte order is the numeric order and which is
//! identical for equal values (`1.0`, `1.00` and `1`), so one key serves
//! equality, range bounds, index order and uniqueness.

use super::numeric::{self, NumericKey};
use super::real;

/// Exponents are biased so the fixed-width hex text orders as the signed value.
const EXPONENT_BIAS: i64 = 1 << 62;
/// Sorts after every digit and every complemented digit.
const NEGATIVE_TERMINATOR: char = '~';

fn complement_hex(text: &str) -> String {
    text.chars()
        .map(|digit| {
            digit
                .to_digit(16)
                .and_then(|value| char::from_digit(15 - value, 16))
                .unwrap_or(digit)
        })
        .collect()
}

/// The key text of a canonical `numeric` key.
#[must_use]
pub(crate) fn numeric_key_text(key: &NumericKey) -> String {
    match key {
        NumericKey::NegInfinity => "0".to_string(),
        NumericKey::Infinity => "4".to_string(),
        NumericKey::NaN => "5".to_string(),
        NumericKey::Finite { mantissa, exponent } => {
            let digits = mantissa.magnitude().to_string();
            if digits == "0" {
                return "2".to_string();
            }
            let digit_count = i64::try_from(digits.len()).unwrap_or(i64::MAX);
            let scientific = exponent.saturating_add(digit_count - 1);
            let biased = u64::try_from(scientific.saturating_add(EXPONENT_BIAS)).unwrap_or(0);
            let exponent_hex = format!("{biased:016x}");
            if mantissa.sign() == num_bigint::Sign::Minus {
                let complemented_digits: String = digits
                    .chars()
                    .map(|digit| char::from(b'9' - (digit as u8 - b'0')))
                    .collect();
                format!(
                    "1{}{complemented_digits}{NEGATIVE_TERMINATOR}",
                    complement_hex(&exponent_hex)
                )
            } else {
                format!("3{exponent_hex}{digits}")
            }
        }
    }
}

/// The key text of a double: equal values (`-0` and `0`) share a key, NaN
/// sorts last, as PostgreSQL orders floats.
#[must_use]
pub(crate) fn float_key_text(value: f64) -> String {
    let bits = if value.is_nan() {
        u64::MAX
    } else {
        let value = if value == 0.0 { 0.0 } else { value };
        if value.is_sign_negative() {
            !value.to_bits()
        } else {
            value.to_bits() | (1 << 63)
        }
    };
    format!("{bits:016x}")
}

fn carrier_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// The index carrier of a stored or probed `numeric` value.
#[must_use]
pub fn numeric_index_carrier(value: &serde_json::Value) -> Option<serde_json::Value> {
    let parsed = numeric::parse_text(&carrier_text(value)?).ok()?;
    Some(serde_json::Value::String(numeric_key_text(
        &parsed.order_key(),
    )))
}

/// Whether a probe constant reaches `numeric_index_carrier` without losing
/// digits. A JSON float has been rounded to a double, so it is trusted only
/// when its shortest text has at most 15 significant digits, which every
/// decimal of that length round-trips through.
#[must_use]
pub fn numeric_probe_is_lossless(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(_) => true,
        serde_json::Value::Number(number) if number.is_i64() || number.is_u64() => true,
        serde_json::Value::Number(number) => {
            let text = number.to_string();
            let mantissa = text.split(['e', 'E']).next().unwrap_or_default();
            let digits = mantissa
                .chars()
                .filter(char::is_ascii_digit)
                .skip_while(|digit| *digit == '0')
                .count();
            digits <= 15
        }
        _ => false,
    }
}

/// The index carrier of a stored `real` value, which is exactly its `f32`.
#[must_use]
pub fn real_stored_index_carrier(value: &serde_json::Value) -> Option<serde_json::Value> {
    let float = match value {
        serde_json::Value::String(text) => f64::from(real::parse_text(text).ok()?),
        serde_json::Value::Number(number) => f64::from(real::from_f64(number.as_f64()?, "").ok()?),
        _ => return None,
    };
    Some(serde_json::Value::String(float_key_text(float)))
}

/// The index carrier of a `real` probe, which SQL compares as a double.
#[must_use]
pub fn real_probe_index_carrier(value: &serde_json::Value) -> Option<serde_json::Value> {
    let float = match value {
        serde_json::Value::String(text) => f64::from(real::parse_text(text).ok()?),
        serde_json::Value::Number(number) => number.as_f64()?,
        _ => return None,
    };
    Some(serde_json::Value::String(float_key_text(float)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> String {
        numeric_index_carrier(&serde_json::Value::String(text.to_string()))
            .and_then(|value| value.as_str().map(str::to_string))
            .expect("numeric key")
    }

    #[test]
    fn should_order_numeric_keys_by_value() {
        // Arrange
        let ascending = [
            "-Infinity",
            "-1000",
            "-10",
            "-2.5",
            "-2",
            "-0.5",
            "0",
            "0.001",
            "1",
            "2",
            "2.5",
            "10",
            "9007199254740992.5",
            "9007199254740993",
            "Infinity",
            "NaN",
        ];

        // Act
        let keys: Vec<String> = ascending.iter().map(|text| key(text)).collect();

        // Assert
        assert!(keys.windows(2).all(|pair| pair[0] < pair[1]), "{keys:?}");
    }

    #[test]
    fn should_key_equal_numeric_values_identically() {
        // Arrange
        let carriers = [
            serde_json::json!("1.0"),
            serde_json::json!("1.00"),
            serde_json::json!(1),
        ];

        // Act
        let keys: Vec<_> = carriers.iter().map(numeric_index_carrier).collect();

        // Assert
        assert!(keys.iter().all(|candidate| *candidate == keys[0]));
        assert_eq!(key("0.0"), key("-0"));
    }

    #[test]
    fn should_order_float_keys_by_value() {
        // Arrange
        let ascending = [
            f64::NEG_INFINITY,
            -2.5,
            -0.5,
            0.0,
            0.5,
            2.5,
            1e300,
            f64::INFINITY,
        ];

        // Act
        let keys: Vec<String> = ascending
            .iter()
            .map(|value| float_key_text(*value))
            .collect();

        // Assert
        assert!(keys.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn should_key_negative_zero_like_zero() {
        // Arrange
        let negative = -0.0_f64;

        // Act
        let key = float_key_text(negative);

        // Assert
        assert_eq!(key, float_key_text(0.0));
    }

    #[test]
    fn should_sort_nan_float_key_after_infinity() {
        // Arrange
        let infinity = float_key_text(f64::INFINITY);

        // Act
        let nan = float_key_text(f64::NAN);

        // Assert
        assert!(infinity < nan);
    }
}
