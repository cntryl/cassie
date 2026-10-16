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

/// Conversions from exact integer NUMERIC tokens stay in the folding/runtime
/// phase. They must never acquire unknown-string input semantics.
pub(super) fn numeric_literal(
    input: &str,
    target: &DataType,
    identity: crate::sql::ast::CastIdentity,
) -> Result<ScalarValue, QueryError> {
    if identity != crate::sql::ast::CastIdentity::Scalar {
        return Err(QueryError::Cassie(crate::app::CassieError::Unsupported(
            "exact NUMERIC literal conversion is not yet supported".into(),
        )));
    }
    if let Some(width) = crate::types::IntegerWidth::from_data_type(target) {
        return super::integers::input(input, width);
    }
    if *target == DataType::Float {
        return crate::types::float_input::parse(input)
            .map(ScalarValue::Float)
            .map_err(|error| QueryError::Cassie(crate::app::CassieError::FloatInput(error)));
    }
    match target {
        DataType::Text | DataType::Varchar { length: None } => {
            return Ok(ScalarValue::Str(input.into()))
        }
        DataType::Varchar {
            length: Some(length),
        } => {
            return Ok(ScalarValue::Str(
                input
                    .chars()
                    .take(usize::try_from(*length).unwrap_or(usize::MAX))
                    .collect(),
            ))
        }
        _ => {}
    }
    Err(QueryError::General(
        "NUMERIC literal conversion requires a supported primitive target".into(),
    ))
}
