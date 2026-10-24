//! JSONB containment operators over Cassie's JSON document carrier.
use serde_json::value::RawValue;

use super::{json_document, BinaryOp, QueryError, ScalarValue};
use crate::app::CassieError;
use crate::types::extended::TypedOperator;

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
    if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) {
        return Ok(ScalarValue::Null);
    }
    let (ScalarValue::Json(left), ScalarValue::Json(right)) = (left, right) else {
        return Err(unsupported_operator(op));
    };
    let left = json_document::parse_value(left)?;
    let right = json_document::parse_value(right)?;
    let result = match op.typed_operator() {
        Some(TypedOperator::ContainsOrEqual) => jsonb_contains_root(&left, &right)?,
        Some(TypedOperator::ContainedOrEqual) => jsonb_contains_root(&right, &left)?,
        _ => return Err(unsupported_operator(op)),
    };
    Ok(ScalarValue::Bool(result))
}

fn jsonb_contains_root(candidate: &RawValue, required: &RawValue) -> Result<bool, QueryError> {
    if kind(candidate) == JsonKind::Array && kind(required).is_scalar() {
        return json_document::parse_array(candidate.get())?
            .iter()
            .try_fold(false, |found, value| {
                if found {
                    Ok(true)
                } else {
                    scalar_equal(value, required)
                }
            });
    }
    contains_node(candidate, required)
}

fn contains_node(candidate: &RawValue, required: &RawValue) -> Result<bool, QueryError> {
    match (kind(candidate), kind(required)) {
        (JsonKind::Object, JsonKind::Object) => {
            let candidate_fields = json_document::parse_object(candidate.get())?.0;
            let required_fields = json_document::parse_object(required.get())?.0;
            for (key, required_value) in required_fields {
                let Some(candidate_value) = candidate_fields.get(&key) else {
                    return Ok(false);
                };
                if !contains_node(candidate_value, &required_value)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        (JsonKind::Array, JsonKind::Array) => {
            let candidate_values = json_document::parse_array(candidate.get())?;
            let required_values = json_document::parse_array(required.get())?;
            for required_value in required_values {
                let mut found = false;
                for candidate_value in &candidate_values {
                    if contains_node(candidate_value, &required_value)? {
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        (JsonKind::Number, JsonKind::Number)
        | (JsonKind::String, JsonKind::String)
        | (JsonKind::Boolean, JsonKind::Boolean)
        | (JsonKind::Null, JsonKind::Null) => scalar_equal(candidate, required),
        _ => Ok(false),
    }
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
