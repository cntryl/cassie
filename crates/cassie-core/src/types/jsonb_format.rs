//! Pure JSONB output and output-size helpers shared by value carriers.

use super::extended::TypeInputError;
use serde_json::Value;

#[doc(hidden)]
pub fn format(value: &Value) -> Result<String, TypeInputError> {
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
    serde_json::to_string(value).map_err(|error| {
        TypeInputError::new(
            "22P02",
            format!("invalid input syntax for type jsonb: {error}"),
        )
    })
}

#[doc(hidden)]
pub fn text_len(value: &Value) -> Result<usize, TypeInputError> {
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

#[doc(hidden)]
pub fn input_text_len(input: &str) -> Result<usize, TypeInputError> {
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
                    && !bytes[index].is_ascii_whitespace()
                    && !matches!(bytes[index], b'[' | b']' | b'{' | b'}' | b':' | b',')
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
    super::extended::numeric::decimal::json_parts(value)?.text_bytes()
}

fn checked_add(left: usize, right: usize) -> Result<usize, TypeInputError> {
    left.checked_add(right).ok_or_else(out_of_range)
}

fn out_of_range() -> TypeInputError {
    TypeInputError::new("22003", "value out of range: overflow".into())
}
