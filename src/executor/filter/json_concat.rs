//! PostgreSQL JSONB top-level concatenation over Cassie's existing JSON carrier.
use serde_json::value::RawValue;

use super::{json_document, QueryError, ScalarValue};
use crate::app::CassieError;

pub(super) fn as_json_document(value: &ScalarValue) -> Result<ScalarValue, QueryError> {
    match value {
        ScalarValue::Null | ScalarValue::Json(_) => Ok(value.clone()),
        ScalarValue::Bool(value) => Ok(ScalarValue::Json(value.to_string())),
        ScalarValue::Int(value) => Ok(ScalarValue::Json(value.to_string())),
        ScalarValue::Float(value) => serde_json::Number::from_f64(*value)
            .map(|number| ScalarValue::Json(number.to_string()))
            .ok_or_else(|| QueryError::General("invalid JSON number".into())),
        ScalarValue::Str(value) => serde_json::to_string(value)
            .map(ScalarValue::Json)
            .map_err(|error| QueryError::General(error.to_string())),
        ScalarValue::Typed(_) => Err(CassieError::Execution(
            "operator does not exist: jsonb || incompatible operands".into(),
        )
        .into()),
    }
}

pub(super) fn concatenate(
    left: &ScalarValue,
    right: &ScalarValue,
) -> Result<ScalarValue, QueryError> {
    if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) {
        return Ok(ScalarValue::Null);
    }
    let (ScalarValue::Json(left), ScalarValue::Json(right)) = (left, right) else {
        return Err(CassieError::Execution(
            "operator does not exist: jsonb || incompatible operands".into(),
        )
        .into());
    };
    let left_value = json_document::parse_value(left)?;
    let right_value = json_document::parse_value(right)?;
    if is_object(&left_value) && is_object(&right_value) {
        let mut fields = json_document::parse_object(left)?.0;
        fields.extend(json_document::parse_object(right)?.0);
        return json_document::format_object(fields).map(ScalarValue::Json);
    }
    let mut values = array_or_singleton(left_value)?;
    values.extend(array_or_singleton(right_value)?);
    json_document::format_array(values).map(ScalarValue::Json)
}

fn is_object(value: &RawValue) -> bool {
    matches!(value.get().trim_start().as_bytes().first(), Some(b'{'))
}

fn array_or_singleton(value: Box<RawValue>) -> Result<Vec<Box<RawValue>>, QueryError> {
    if matches!(value.get().trim_start().as_bytes().first(), Some(b'[')) {
        json_document::parse_array(value.get())
    } else {
        Ok(vec![value])
    }
}
