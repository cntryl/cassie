//! `left op ANY|SOME|ALL (array)` over the JSON or text array carrier.
//!
//! A NULL array yields NULL. Otherwise ANY is true when one element compares
//! true and ALL is false when one compares false; an undecided NULL
//! comparison leaves the result unknown, and the empty array yields false for
//! ANY and true for ALL.
use super::{binary_scalar, BinaryOp, PredicateResult, QueryError, ScalarValue};

pub(super) fn evaluate(
    left: &ScalarValue,
    comparison: &BinaryOp,
    all: bool,
    right: &ScalarValue,
) -> Result<ScalarValue, QueryError> {
    let Some(elements) = elements(right)? else {
        return Ok(ScalarValue::Null);
    };
    let mut result = if all {
        PredicateResult::True
    } else {
        PredicateResult::False
    };
    for element in &elements {
        let element = coerced(left, element);
        match (
            all,
            binary_scalar(left, comparison, &element)?.predicate_result()?,
        ) {
            (false, PredicateResult::True) => return Ok(ScalarValue::Bool(true)),
            (true, PredicateResult::False) => return Ok(ScalarValue::Bool(false)),
            (_, PredicateResult::Unknown) => result = PredicateResult::Unknown,
            _ => {}
        }
    }
    Ok(result.into_scalar())
}

fn elements(array: &ScalarValue) -> Result<Option<Vec<ScalarValue>>, QueryError> {
    let json = match array {
        ScalarValue::Null => return Ok(None),
        ScalarValue::Json(text) | ScalarValue::Str(text) if text.trim_start().starts_with('[') => {
            serde_json::from_str::<serde_json::Value>(text).map_err(|error| {
                crate::app::CassieError::ArrayInput(format!("malformed array literal: {error}"))
            })?
        }
        ScalarValue::Str(text) => {
            crate::types::array::parse_text_array(text, &crate::types::DataType::Text)
                .map_err(|error| crate::app::CassieError::ArrayInput(error.to_string()))?
        }
        _ => {
            return Err(crate::app::CassieError::Execution(
                "op ANY/ALL (array) requires array on right side".into(),
            )
            .into())
        }
    };
    let serde_json::Value::Array(values) = json else {
        return Err(crate::app::CassieError::Execution(
            "op ANY/ALL (array) requires array on right side".into(),
        )
        .into());
    };
    Ok(Some(values.iter().map(element).collect()))
}

/// An element of an untyped text array literal takes the left operand's
/// numeric type, as the unknown literal is resolved against it.
fn coerced(left: &ScalarValue, element: &ScalarValue) -> ScalarValue {
    let ScalarValue::Str(text) = element else {
        return element.clone();
    };
    match left {
        ScalarValue::Int(_) => text
            .trim()
            .parse::<i64>()
            .map_or_else(|_| element.clone(), ScalarValue::Int),
        ScalarValue::Float(_) => text
            .trim()
            .parse::<f64>()
            .map_or_else(|_| element.clone(), ScalarValue::Float),
        _ => element.clone(),
    }
}

fn element(value: &serde_json::Value) -> ScalarValue {
    match value {
        serde_json::Value::Null => ScalarValue::Null,
        serde_json::Value::Bool(value) => ScalarValue::Bool(*value),
        serde_json::Value::Number(number) => number.as_i64().map_or_else(
            || {
                number
                    .as_f64()
                    .map_or(ScalarValue::Null, ScalarValue::Float)
            },
            ScalarValue::Int,
        ),
        serde_json::Value::String(text) => ScalarValue::Str(text.clone()),
        other => ScalarValue::Json(other.to_string()),
    }
}
