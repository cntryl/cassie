//! Selected JSONB key-existence operators over Cassie's existing JSON carrier.
use super::{BinaryOp, QueryError, ScalarValue};
use crate::app::CassieError;
use crate::types::{array, DataType};
use serde_json::Value as JsonValue;

#[cfg(test)]
#[path = "json_exists_owned_tests.rs"]
mod owned_tests;

pub(super) fn evaluate_controlled(
    left: &ScalarValue,
    op: &BinaryOp,
    right: &ScalarValue,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<Option<ScalarValue>, QueryError> {
    if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) {
        return Ok(Some(ScalarValue::Null));
    }
    let ScalarValue::Json(document) = left else {
        return Ok(None);
    };
    let Some(document) =
        crate::runtime::accounted::json_arena::parse_existence_candidate(document, controls)?
    else {
        return Ok(None);
    };
    match op {
        BinaryOp::JsonbKeyExists => {
            let ScalarValue::Str(key) = right else {
                return Ok(None);
            };
            Ok(Some(ScalarValue::Bool(document.contains(key)?)))
        }
        BinaryOp::JsonbKeyExistsAny | BinaryOp::JsonbKeyExistsAll => {
            let all = matches!(op, BinaryOp::JsonbKeyExistsAll);
            if let ScalarValue::Array(keys) = right {
                for value in keys.elements() {
                    controls.check_interruption()?;
                    if !matches!(
                        value,
                        crate::types::Value::String(_) | crate::types::Value::Null
                    ) {
                        return Ok(None);
                    }
                }
                let values = keys.elements();
                let exists = lookup_keys(
                    values.len(),
                    |index| {
                        Ok(match &values[index] {
                            crate::types::Value::String(text) => Some(text.as_str()),
                            _ => None,
                        })
                    },
                    all,
                    controls,
                    |key| document.contains(key),
                )?;
                return Ok(Some(ScalarValue::Bool(exists)));
            }
            let (ScalarValue::Str(text) | ScalarValue::Json(text)) = right else {
                return Ok(None);
            };
            let parsed = match super::json_delete::owned_path::parse(text, controls) {
                Ok(path) => path,
                Err(QueryError::Cassie(CassieError::ArrayInput(_))) => return Ok(None),
                Err(error) => return Err(error),
            };
            if let Some(path) = parsed {
                super::json_delete::after_owned_path(controls)?;
                let values = path.values();
                let exists = lookup_keys(
                    values.len(),
                    |index| Ok(values[index].as_deref()),
                    all,
                    controls,
                    |key| document.contains(key),
                )?;
                return Ok(Some(ScalarValue::Bool(exists)));
            }
            if let Some(path) =
                crate::runtime::accounted::json_arena::parse_text_array_candidate(text, controls)?
            {
                super::json_delete::after_owned_path(controls)?;
                let exists = lookup_keys(
                    path.len(),
                    |index| path.get(index),
                    all,
                    controls,
                    |key| document.contains(key),
                )?;
                return Ok(Some(ScalarValue::Bool(exists)));
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

fn lookup_keys<'a>(
    length: usize,
    mut key: impl FnMut(usize) -> Result<Option<&'a str>, CassieError>,
    all: bool,
    controls: &crate::runtime::QueryExecutionControls,
    mut contains: impl FnMut(&str) -> Result<bool, CassieError>,
) -> Result<bool, CassieError> {
    for index in 0..length {
        controls.check_interruption()?;
        if let Some(key) = key(index)? {
            if contains(key)? != all {
                return Ok(!all);
            }
        }
    }
    Ok(all)
}

pub(super) fn evaluate(
    left: &ScalarValue,
    op: &BinaryOp,
    right: &ScalarValue,
) -> Result<ScalarValue, QueryError> {
    if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) {
        return Ok(ScalarValue::Null);
    }
    let ScalarValue::Json(document) = left else {
        return Err(operator_input_error(op));
    };
    let document = serde_json::from_str::<JsonValue>(document)
        .map_err(|error| QueryError::General(format!("invalid JSON document: {error}")))?;
    let exists = |key: &str| match &document {
        JsonValue::Object(object) => object.contains_key(key),
        JsonValue::Array(values) => values.iter().any(|value| value.as_str() == Some(key)),
        JsonValue::String(value) => value == key,
        JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) => false,
    };

    match op {
        BinaryOp::JsonbKeyExists => {
            let ScalarValue::Str(key) = right else {
                return Err(operator_input_error(op));
            };
            Ok(ScalarValue::Bool(exists(key)))
        }
        BinaryOp::JsonbKeyExistsAny | BinaryOp::JsonbKeyExistsAll => {
            let keys = text_array(right, op)?;
            let all = matches!(op, BinaryOp::JsonbKeyExistsAll);
            Ok(ScalarValue::Bool(if all {
                keys.iter().filter_map(JsonValue::as_str).all(exists)
            } else {
                keys.iter().filter_map(JsonValue::as_str).any(exists)
            }))
        }
        _ => Err(operator_input_error(op)),
    }
}

fn text_array(value: &ScalarValue, op: &BinaryOp) -> Result<Vec<JsonValue>, QueryError> {
    if let ScalarValue::Array(array) = value {
        return array
            .elements()
            .iter()
            .map(|value| match value {
                crate::types::Value::String(text) => Ok(JsonValue::String(text.clone())),
                crate::types::Value::Null => Ok(JsonValue::Null),
                _ => Err(operator_input_error(op)),
            })
            .collect();
    }
    let (ScalarValue::Json(text) | ScalarValue::Str(text)) = value else {
        return Err(operator_input_error(op));
    };
    array::parse_text_array(text, &DataType::Text)
        .map_err(|error| QueryError::Cassie(CassieError::ArrayInput(error.to_string())))?
        .as_array()
        .cloned()
        .ok_or_else(|| operator_input_error(op))
}

fn operator_input_error(op: &BinaryOp) -> QueryError {
    let signature = match op {
        BinaryOp::JsonbKeyExists => "json ? text",
        BinaryOp::JsonbKeyExistsAny => "json ?| text[]",
        BinaryOp::JsonbKeyExistsAll => "json ?& text[]",
        _ => "json operator with incompatible operands",
    };
    CassieError::Execution(format!("operator does not exist: {signature}")).into()
}
