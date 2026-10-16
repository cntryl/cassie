//! Source-aware scalar casts, shared by runtime and semantic constants.
use super::{cast_scalar, DataType, QueryError, ScalarValue};

pub(super) fn scalar(
    value: &ScalarValue,
    target: &DataType,
    source: Option<&DataType>,
) -> Result<ScalarValue, QueryError> {
    if *target == DataType::Float {
        match value {
            ScalarValue::Float(value) => return Ok(ScalarValue::Float(*value)),
            ScalarValue::Str(text) => {
                return crate::types::float_input::parse(text)
                    .map(ScalarValue::Float)
                    .map_err(|error| {
                        QueryError::Cassie(crate::app::CassieError::FloatInput(error))
                    })
            }
            _ => {}
        }
    }
    if matches!(target, DataType::Text | DataType::Varchar { .. }) {
        if source == Some(&DataType::Float) {
            if let ScalarValue::Float(value) = value {
                return cast_scalar(
                    &ScalarValue::Str(crate::types::float_text::format(*value)),
                    target,
                );
            }
        }
        if let ScalarValue::Str(text) = value {
            let formatted = match source {
                Some(DataType::Timestamp) => Some(crate::types::temporal::timestamp_text(text)),
                Some(DataType::Time) => Some(crate::types::temporal::time_text(text)),
                _ => None,
            };
            if let Some(formatted) = formatted {
                return cast_scalar(
                    &ScalarValue::Str(formatted.map_err(QueryError::General)?),
                    target,
                );
            }
        }
    }
    cast_scalar(value, target)
}
