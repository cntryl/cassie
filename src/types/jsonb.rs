//! Canonical JSONB text over the existing exact JSON document carrier.
use super::extended::TypeInputError;
use serde_json::Value;

pub(crate) fn canonical(input: &str) -> Result<String, TypeInputError> {
    let value = serde_json::from_str(input).map_err(|error| invalid_json(&error))?;
    format(&value)
}

pub(crate) fn format(value: &Value) -> Result<String, TypeInputError> {
    match value {
        Value::Null => Ok("null".into()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) => {
            super::extended::numeric::parse_text(value.as_str()).map(|number| number.to_string())
        }
        Value::String(value) => quoted(value),
        Value::Array(values) => {
            let values = values.iter().map(format).collect::<Result<Vec<_>, _>>()?;
            Ok(format!("[{}]", values.join(", ")))
        }
        Value::Object(fields) => {
            let mut fields = fields.iter().collect::<Vec<_>>();
            fields.sort_unstable_by(|(left, _), (right, _)| {
                left.len()
                    .cmp(&right.len())
                    .then_with(|| left.as_bytes().cmp(right.as_bytes()))
            });
            let fields = fields
                .into_iter()
                .map(|(key, value)| Ok(format!("{}: {}", quoted(key)?, format(value)?)))
                .collect::<Result<Vec<_>, TypeInputError>>()?;
            Ok(format!("{{{}}}", fields.join(", ")))
        }
    }
}

fn quoted(value: &str) -> Result<String, TypeInputError> {
    if value.contains('\0') {
        return Err(TypeInputError::new(
            "22P05",
            "unsupported Unicode escape sequence: \\u0000".into(),
        ));
    }
    serde_json::to_string(value).map_err(|error| invalid_json(&error))
}

fn invalid_json(error: &serde_json::Error) -> TypeInputError {
    TypeInputError::new(
        "22P02",
        format!("invalid input syntax for type jsonb: {error}"),
    )
}

/// Bound canonical text before wire encoding allocates its output or scratch.
pub(crate) fn text_len(value: &Value) -> Result<usize, TypeInputError> {
    match value {
        Value::Null => Ok(4),
        Value::Bool(_) => Ok(5),
        Value::String(value) => quoted_len(value),
        Value::Number(value) => number_len(value.as_str()),
        Value::Array(values) => values.iter().try_fold(2, |bytes, value| {
            checked_add(checked_add(bytes, 2)?, text_len(value)?)
        }),
        Value::Object(values) => values.iter().try_fold(2, |bytes, (key, value)| {
            checked_add(
                checked_add(checked_add(bytes, 4)?, quoted_len(key)?)?,
                text_len(value)?,
            )
        }),
    }
}

pub(crate) fn input_text_len(input: &str) -> Result<usize, TypeInputError> {
    let bytes = input.as_bytes();
    let mut bound = input.len().checked_mul(2).ok_or_else(out_of_range)?;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                index += 1;
                while index < bytes.len() && bytes[index] != b'"' {
                    if bytes[index] == b'\\' {
                        index += 1;
                    }
                    index += 1;
                }
            }
            b'-' | b'0'..=b'9' => {
                let start = index;
                while index < bytes.len()
                    && matches!(bytes[index], b'-' | b'+' | b'0'..=b'9' | b'.' | b'e' | b'E')
                {
                    index += 1;
                }
                let token = &input[start..index];
                bound = checked_add(bound, number_len(token)?.saturating_sub(token.len()))?;
                continue;
            }
            _ => {}
        }
        index += 1;
    }
    Ok(bound)
}

fn quoted_len(value: &str) -> Result<usize, TypeInputError> {
    value.bytes().try_fold(2, |bytes, byte| {
        checked_add(
            bytes,
            match byte {
                0..=31 => 6,
                b'"' | b'\\' => 2,
                _ => 1,
            },
        )
    })
}

fn number_len(value: &str) -> Result<usize, TypeInputError> {
    let (mantissa, exponent) = value
        .find(['e', 'E'])
        .map_or((value, "0"), |index| (&value[..index], &value[index + 1..]));
    let exponent = exponent.parse::<i64>().map_err(|_| out_of_range())?;
    let digits = i64::try_from(mantissa.bytes().filter(u8::is_ascii_digit).count())
        .map_err(|_| out_of_range())?;
    let fraction = mantissa
        .find('.')
        .map_or(0, |index| mantissa.len() - index - 1);
    let scale = i64::try_from(fraction)
        .map_err(|_| out_of_range())?
        .checked_sub(exponent)
        .ok_or_else(out_of_range)?;
    let digits = if scale < 0 {
        digits.checked_sub(scale)
    } else {
        Some(digits.max(scale.saturating_add(1)))
    }
    .ok_or_else(out_of_range)?;
    let bytes = digits
        .checked_add(i64::from(scale > 0) + i64::from(value.starts_with('-')))
        .ok_or_else(out_of_range)?;
    if bytes > 147_457 {
        return Err(out_of_range());
    }
    usize::try_from(bytes).map_err(|_| out_of_range())
}

fn checked_add(left: usize, right: usize) -> Result<usize, TypeInputError> {
    left.checked_add(right).ok_or_else(out_of_range)
}

fn out_of_range() -> TypeInputError {
    TypeInputError::new("22003", "JSONB numeric value is out of range".into())
}

#[cfg(test)]
mod tests {
    use super::{canonical, input_text_len, text_len};

    #[test]
    fn should_preserve_exact_jsonb_numbers_given_duplicate_keys_when_text_is_canonicalized() {
        // Arrange
        let input = r#"{"n":0,"n":123456789012345678901234567890.125,"v":[1.00,1e2]}"#;
        // Act
        let actual = canonical(input).expect("canonical JSONB");
        // Assert
        assert_eq!(
            actual,
            r#"{"n": 123456789012345678901234567890.125, "v": [1.00, 100]}"#
        );
    }

    #[test]
    fn should_bound_jsonb_text_given_expanding_numbers_when_wire_memory_is_admitted() {
        // Arrange
        let input = r#"{"n":1e100,"s":"1e9999999999999999999999999"}"#;
        let value = serde_json::from_str(input).expect("exact document");
        // Act
        let output = canonical(input).expect("canonical JSONB");
        let value_bound = text_len(&value).expect("value bound");
        let text_bound = input_text_len(input).expect("text bound");
        // Assert
        assert!(value_bound >= output.len());
        assert!(text_bound >= output.len());
    }

    #[test]
    fn should_reject_jsonb_unicode_null_given_escaped_input_when_text_is_canonicalized() {
        // Arrange
        let input = r#"{"s":"\u0000"}"#;
        // Act
        let error = canonical(input).expect_err("JSONB cannot contain a Unicode null");
        // Assert
        assert_eq!(error.sqlstate, "22P05");
    }
}
