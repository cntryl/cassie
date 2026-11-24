pub(crate) use crate::types::semantic::*;

use crate::app::CassieError;
use crate::runtime::QueryExecutionControls;
use crate::types::{DataType, Value};

/// Declared identities select semantic adapters; a JSON-looking TEXT stays TEXT.
pub(crate) fn typed_value(
    value: &Value,
    data_type: Option<&DataType>,
    controls: Option<&QueryExecutionControls>,
) -> Result<SemanticValue, CassieError> {
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    match (value, data_type) {
        (Value::Null, _) => Ok(SemanticValue::Null),
        (Value::String(text), Some(DataType::Char { .. })) => Ok(SemanticValue::String(
            crate::types::char_text::canonical_char_text(text).to_owned(),
        )),
        (Value::String(text), Some(DataType::Jsonb)) => {
            JsonbKey::from_text(text, controls).map(SemanticValue::Jsonb)
        }
        (Value::Json(value), Some(DataType::Jsonb)) => {
            JsonbKey::from_json(value, controls).map(SemanticValue::Jsonb)
        }
        (_, Some(DataType::Array(element))) if jsonb_type(element) => jsonb_array(value, controls),
        (_, Some(DataType::Jsonb)) => Err(crate::types::extended::TypeInputError::new(
            "22P02",
            "invalid input representation for type jsonb".into(),
        )
        .into()),
        _ => Ok(SemanticValue::from_value(value)),
    }
}

pub(crate) fn jsonb_type(data_type: &DataType) -> bool {
    match data_type {
        DataType::Jsonb => true,
        DataType::Array(element) => jsonb_type(element),
        _ => false,
    }
}

pub(super) fn jsonb_array(
    value: &Value,
    controls: Option<&QueryExecutionControls>,
) -> Result<SemanticValue, CassieError> {
    use crate::runtime::accounted::json;
    use crate::types::SqlArray;
    if let Value::Array(array) = value {
        return JsonbArrayKey::build(array, controls, |value| {
            typed_value(value, Some(&DataType::Jsonb), controls)
        })
        .map(SemanticValue::JsonbArray);
    }
    let bytes = match value {
        Value::String(text) => json::serialized_decode_bytes(text.len())?.checked_mul(3),
        Value::Json(value) => json::retained_bytes(value)?.checked_mul(4),
        _ => return Err(invalid_array("invalid JSONB[] representation".into())),
    }
    .and_then(|bytes| bytes.checked_add(256))
    .ok_or_else(|| CassieError::ResourceLimit("JSONB array adaptation overflow".into()))?;
    let _workspace = controls
        .map(|controls| controls.reserve_query_memory(bytes))
        .transpose()?;
    let array = match value {
        Value::Json(value) => {
            SqlArray::from_nested_json_typed(value, &DataType::Jsonb).map_err(invalid_array)?
        }
        Value::String(text) => {
            let parsed =
                crate::types::array::parse_text_array_with_dimensions(text, &DataType::Jsonb)
                    .map_err(|error| invalid_array(error.to_string()))?;
            let elements = parsed
                .flat_elements
                .into_iter()
                .map(|element| match element {
                    crate::types::array::ParsedTextArrayElement::SqlNull => Value::Null,
                    crate::types::array::ParsedTextArrayElement::Value(value) => Value::Json(value),
                })
                .collect();
            SqlArray::new(parsed.dimensions, elements).map_err(invalid_array)?
        }
        _ => unreachable!("representation checked before admission"),
    };
    JsonbArrayKey::build(&array, controls, |value| {
        typed_value(value, Some(&DataType::Jsonb), controls)
    })
    .map(SemanticValue::JsonbArray)
}
fn invalid_array(message: String) -> CassieError {
    crate::types::extended::TypeInputError::new("22P02", message).into()
}

#[cfg(test)]
mod tests;
