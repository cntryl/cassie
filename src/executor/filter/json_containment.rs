//! JSONB containment operators over Cassie's JSON document carrier.
use serde_json::value::RawValue;

use super::{json_document, BinaryOp, QueryError, ScalarValue};
use crate::app::CassieError;
use crate::types::extended::TypedOperator;

#[path = "json_containment_walk.rs"]
mod walk;

pub(super) fn evaluate_nested(
    candidate: &str,
    required: &str,
    controls: &crate::runtime::QueryExecutionControls,
    after_scalar: &mut impl FnMut(bool),
) -> Result<bool, QueryError> {
    walk::evaluate_nested(candidate, required, controls, after_scalar)
}

pub(super) const fn handles(op: &BinaryOp) -> bool {
    matches!(
        op.typed_operator(),
        Some(TypedOperator::ContainsOrEqual | TypedOperator::ContainedOrEqual)
    )
}

pub(super) fn evaluate(
    op: &BinaryOp,
    left: &ScalarValue,
    right: &ScalarValue,
) -> Result<ScalarValue, QueryError> {
    evaluate_with_controls(op, left, right, None)
}

pub(super) fn evaluate_with_controls(
    op: &BinaryOp,
    left: &ScalarValue,
    right: &ScalarValue,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<ScalarValue, QueryError> {
    evaluate_observing(op, left, right, controls, |_| {})
}

fn evaluate_observing(
    op: &BinaryOp,
    left: &ScalarValue,
    right: &ScalarValue,
    controls: Option<&crate::runtime::QueryExecutionControls>,
    after_scalar: impl FnMut(bool),
) -> Result<ScalarValue, QueryError> {
    if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) {
        return Ok(ScalarValue::Null);
    }
    let (ScalarValue::Json(left), ScalarValue::Json(right)) = (left, right) else {
        return Err(unsupported_operator(op));
    };
    let result = match op.typed_operator() {
        Some(TypedOperator::ContainsOrEqual) => {
            walk::evaluate(left, right, controls, after_scalar)?
        }
        Some(TypedOperator::ContainedOrEqual) => {
            walk::evaluate(right, left, controls, after_scalar)?
        }
        _ => return Err(unsupported_operator(op)),
    };
    Ok(ScalarValue::Bool(result))
}

fn scalar_equal(left: &RawValue, right: &RawValue) -> Result<bool, QueryError> {
    match (kind(left), kind(right)) {
        (JsonKind::Number, JsonKind::Number) => {
            let left = crate::types::extended::numeric::parse_text(left.get())
                .map_err(|error| QueryError::General(error.to_string()))?;
            let right = crate::types::extended::numeric::parse_text(right.get())
                .map_err(|error| QueryError::General(error.to_string()))?;
            Ok(left.compare(&right).is_eq())
        }
        (JsonKind::String, JsonKind::String) => {
            let left = serde_json::from_str::<String>(left.get())
                .map_err(|error| QueryError::General(error.to_string()))?;
            let right = serde_json::from_str::<String>(right.get())
                .map_err(|error| QueryError::General(error.to_string()))?;
            Ok(left == right)
        }
        (JsonKind::Boolean, JsonKind::Boolean) | (JsonKind::Null, JsonKind::Null) => {
            Ok(left.get().trim() == right.get().trim())
        }
        _ => Ok(false),
    }
}

fn unsupported_operator(op: &BinaryOp) -> QueryError {
    let symbol = crate::sql::binder::json_containment::operator_symbol(op).unwrap_or("?");
    QueryError::Cassie(CassieError::Execution(format!(
        "operator does not exist: jsonb {symbol} incompatible operands"
    )))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JsonKind {
    Object,
    Array,
    String,
    Number,
    Boolean,
    Null,
    Other,
}

impl JsonKind {
    const fn is_scalar(self) -> bool {
        matches!(
            self,
            Self::String | Self::Number | Self::Boolean | Self::Null
        )
    }
}

fn kind(value: &RawValue) -> JsonKind {
    match value.get().trim_start().as_bytes().first() {
        Some(b'{') => JsonKind::Object,
        Some(b'[') => JsonKind::Array,
        Some(b'"') => JsonKind::String,
        Some(b'-' | b'0'..=b'9') => JsonKind::Number,
        Some(b't' | b'f') => JsonKind::Boolean,
        Some(b'n') => JsonKind::Null,
        _ => JsonKind::Other,
    }
}

#[cfg(test)]
#[path = "json_containment_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "json_containment_string_tests.rs"]
mod string_tests;

#[cfg(test)]
#[path = "json_containment_number_tests.rs"]
mod number_tests;

#[cfg(test)]
#[path = "json_containment_root_object_tests.rs"]
mod root_object_tests;

#[cfg(test)]
#[path = "json_containment_pair_tests.rs"]
mod pair_tests;

#[cfg(test)]
#[path = "json_containment_nested_object_tests.rs"]
mod nested_object_tests;
