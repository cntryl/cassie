//! JSON field and path extraction without rounding document numbers.
use super::{json_document, QueryError, ScalarValue};
use crate::app::CassieError;
use crate::types::{array, DataType};
use serde_json::value::RawValue;

pub(super) fn evaluate(
    document: &ScalarValue,
    selector: &ScalarValue,
    path: bool,
    as_text: bool,
    jsonb: bool,
) -> Result<ScalarValue, QueryError> {
    if matches!(document, ScalarValue::Null) || matches!(selector, ScalarValue::Null) {
        return Ok(ScalarValue::Null);
    }
    let ScalarValue::Json(document) = document else {
        return Err(operator_error());
    };
    if path {
        if let ScalarValue::Array(array) = selector {
            let segments = array
                .elements()
                .iter()
                .map(|value| match value {
                    crate::types::Value::String(text) => Ok(Some(text.as_str())),
                    crate::types::Value::Null => Ok(None),
                    _ => Err(operator_error()),
                })
                .collect::<Result<Vec<_>, _>>()?;
            return evaluate_path(document, segments, as_text, jsonb, None);
        }
    }
    let mut value = json_document::parse_value(document)?;
    if path {
        let (ScalarValue::Str(text) | ScalarValue::Json(text)) = selector else {
            return Err(operator_error());
        };
        let segments = array::parse_text_array(text, &DataType::Text)
            .map_err(|error| CassieError::ArrayInput(error.to_string()))?;
        let Some(segments) = segments.as_array() else {
            return Err(operator_error());
        };
        for segment in segments {
            let Some(segment) = segment.as_str() else {
                return Ok(ScalarValue::Null);
            };
            let Some(next) = extract(&value, segment, true)? else {
                return Ok(ScalarValue::Null);
            };
            value = next;
        }
    } else {
        let next = match selector {
            ScalarValue::Str(key) => extract(&value, key, false)?,
            ScalarValue::Int(index) => {
                if !value.get().trim_start().starts_with(['{', '[']) && matches!(index, 0 | -1) {
                    Some(value)
                } else {
                    array_element(&value, *index)?
                }
            }
            _ => return Err(operator_error()),
        };
        let Some(next) = next else {
            return Ok(ScalarValue::Null);
        };
        value = next;
    }
    output(&value, as_text, jsonb)
}

pub(super) fn evaluate_path<'a>(
    document: &str,
    segments: impl IntoIterator<Item = Option<&'a str>>,
    as_text: bool,
    jsonb: bool,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<ScalarValue, QueryError> {
    let mut value = json_document::parse_value(document)?;
    for segment in segments {
        if let Some(controls) = controls {
            controls.check_interruption()?;
        }
        let Some(segment) = segment else {
            return Ok(ScalarValue::Null);
        };
        let Some(next) = extract(&value, segment, true)? else {
            return Ok(ScalarValue::Null);
        };
        value = next;
    }
    output(&value, as_text, jsonb)
}

fn output(value: &RawValue, as_text: bool, jsonb: bool) -> Result<ScalarValue, QueryError> {
    if !as_text {
        return if jsonb {
            json_document::format_value(value)
        } else {
            Ok(value.get().to_owned())
        }
        .map(ScalarValue::Json);
    }
    match value.get().trim_start().as_bytes().first() {
        Some(b'n') => Ok(ScalarValue::Null),
        Some(b'"') => serde_json::from_str::<String>(value.get())
            .map(ScalarValue::Str)
            .map_err(|error| QueryError::General(error.to_string())),
        _ => if jsonb {
            json_document::format_value(value)
        } else {
            Ok(value.get().to_owned())
        }
        .map(ScalarValue::Str),
    }
}

fn extract(value: &RawValue, key: &str, path: bool) -> Result<Option<Box<RawValue>>, QueryError> {
    match value.get().trim_start().as_bytes().first() {
        Some(b'{') => json_document::object_field(value.get(), key),
        Some(b'[') if path => match key
            .trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c'])
            .parse::<i32>()
        {
            Ok(index) => array_element(value, i64::from(index)),
            Err(_) => Ok(None),
        },
        _ => Ok(None),
    }
}

fn array_element(value: &RawValue, index: i64) -> Result<Option<Box<RawValue>>, QueryError> {
    if !value.get().trim_start().starts_with('[') {
        return Ok(None);
    }
    let mut values = json_document::parse_array(value.get())?;
    let length = i64::try_from(values.len()).unwrap_or(i64::MAX);
    let index = if index < 0 {
        index.checked_add(length)
    } else {
        Some(index)
    };
    let Some(index) = index.and_then(|index| usize::try_from(index).ok()) else {
        return Ok(None);
    };
    if index >= values.len() {
        return Ok(None);
    }
    Ok(Some(values.remove(index)))
}

fn operator_error() -> QueryError {
    CassieError::Execution(
        "operator does not exist: JSON extraction with incompatible operands".into(),
    )
    .into()
}
