//! Selected JSONB key-existence operators over Cassie's existing JSON carrier.
use super::{BinaryOp, QueryError, ScalarValue};
use crate::app::CassieError;
use crate::types::{array, DataType};
use serde_json::Value as JsonValue;

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
