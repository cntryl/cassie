//! Selected one-dimensional SQL TEXT[] input over the logical array carrier.
use super::{QueryError, ScalarValue};
use crate::app::CassieError;
use crate::types::array::TextArrayError;

pub(super) fn cast(value: &ScalarValue) -> Result<ScalarValue, QueryError> {
    let ScalarValue::Str(input) = value else {
        return Err(CassieError::Unsupported(
            "TEXT[] conversion from this scalar carrier remains pending under #793".into(),
        )
        .into());
    };
    if input
        .trim_start()
        .strip_prefix('[')
        .and_then(|rest| rest.chars().next())
        .is_some_and(|ch| ch.is_ascii_digit() || ch == '+' || ch == '-' || ch.is_whitespace())
    {
        return Err(CassieError::Unsupported(
            "explicit array bounds remain pending under #793".into(),
        )
        .into());
    }
    if !input.trim_start().starts_with('{') {
        return Err(CassieError::ArrayInput(
            "malformed array literal: array value must start with '{'".into(),
        )
        .into());
    }
    crate::types::array::parse_text_array(input, &crate::types::DataType::Text)
        .map(|value| ScalarValue::Json(value.to_string()))
        .map_err(|error| match error {
            TextArrayError::Invalid(message) => CassieError::ArrayInput(message).into(),
            TextArrayError::UnsupportedJsonDocumentNull => {
                CassieError::Unsupported(error.to_string()).into()
            }
        })
}
