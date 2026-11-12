//! Selected one-dimensional SQL TEXT[] input over the logical array carrier.
use super::{QueryError, ScalarValue};
use crate::app::CassieError;
use crate::types::{array::TextArrayError, DataType};

pub(super) fn cast(
    value: &ScalarValue,
    element_type: &DataType,
) -> Result<ScalarValue, QueryError> {
    if let ScalarValue::Array(array) = value {
        if array.is_empty() {
            return Ok(value.clone());
        }
    }
    if let ScalarValue::Array(array) = value {
        return cast_elements(array, element_type);
    }
    let ScalarValue::Str(input) = value else {
        return Err(QueryError::General(
            "cannot cast scalar value to ARRAY".into(),
        ));
    };
    let mut element_type = element_type;
    while let DataType::Array(inner) = element_type {
        element_type = inner;
    }
    let parsed = crate::types::array::parse_sql_text_array(input, element_type).map_err(
        |error| -> QueryError {
            match error {
                TextArrayError::Invalid(message) => CassieError::ArrayInput(message).into(),
                TextArrayError::TypeInput(error) => CassieError::TypeInput(error).into(),
                TextArrayError::UnsupportedJsonDocumentNull => {
                    CassieError::Unsupported(error.to_string()).into()
                }
            }
        },
    )?;
    parsed
        .into_sql_array(element_type)
        .map(ScalarValue::Array)
        .map_err(|message| CassieError::ArrayInput(message).into())
}

/// Converts every element of an existing array to the target element type,
/// preserving its dimensions and SQL NULL elements (`ARRAY[1.0]::real[]`).
fn cast_elements(
    array: &crate::types::SqlArray,
    element_type: &DataType,
) -> Result<ScalarValue, QueryError> {
    let mut element_type = element_type;
    while let DataType::Array(inner) = element_type {
        element_type = inner;
    }
    let elements = array
        .elements()
        .iter()
        .map(|element| super::cast_value(element, element_type))
        .collect::<Result<Vec<_>, _>>()?;
    crate::types::SqlArray::new(array.dimensions().to_vec(), elements)
        .map(ScalarValue::Array)
        .map_err(|message| CassieError::ArrayInput(message).into())
}
