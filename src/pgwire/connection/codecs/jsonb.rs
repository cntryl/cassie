//! PostgreSQL JSONB's versioned document format.
use super::{invalid_data, value_to_text, Value};
use crate::types::extended::TypeInputError;
use std::io;

pub(super) fn text(value: &Value) -> String {
    canonical_value(value).unwrap_or_else(|_| value_to_text(value, 114))
}

fn canonical_value(value: &Value) -> Result<String, TypeInputError> {
    match value {
        Value::Json(value) => crate::types::jsonb::format(value),
        Value::String(value) => crate::types::jsonb::canonical(value),
        _ => crate::types::jsonb::canonical(&value_to_text(value, 114)),
    }
}

pub(super) fn encode(value: &Value) -> io::Result<Vec<u8>> {
    let mut document = vec![1];
    document.extend_from_slice(canonical_value(value).map_err(input_error)?.as_bytes());
    Ok(document)
}

pub(super) fn decode(parameter: &[u8]) -> io::Result<Value> {
    let Some((&version, document)) = parameter.split_first() else {
        return Err(invalid_data("jsonb"));
    };
    if version != 1 {
        return Err(input_error(TypeInputError::new(
            "XX000",
            format!("unsupported jsonb version number {version}"),
        )));
    }
    let text = std::str::from_utf8(document).map_err(|_| {
        input_error(TypeInputError::new(
            "22021",
            "invalid byte sequence for encoding UTF8".into(),
        ))
    })?;
    crate::types::jsonb::canonical(text).map_err(input_error)?;
    serde_json::from_str(text)
        .map(Value::Json)
        .map_err(|_| invalid_data("jsonb"))
}

fn input_error(error: TypeInputError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}
