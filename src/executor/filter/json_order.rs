//! JSON document comparison with exact decimal numbers and JSONB type ordering.
use serde_json::Value;
use std::cmp::Ordering;

pub(super) fn evaluate(
    left: &super::ScalarValue,
    op: crate::sql::ast::RowComparisonOp,
    right: &super::ScalarValue,
) -> Result<super::ScalarValue, super::QueryError> {
    use super::ScalarValue;
    use crate::sql::ast::RowComparisonOp;
    let left_null = matches!(left, ScalarValue::Null);
    let right_null = matches!(right, ScalarValue::Null);
    if left_null || right_null {
        return Ok(match op {
            RowComparisonOp::IsDistinctFrom => ScalarValue::Bool(left_null != right_null),
            RowComparisonOp::IsNotDistinctFrom => ScalarValue::Bool(left_null == right_null),
            _ => ScalarValue::Null,
        });
    }
    let ordering = left
        .as_str()
        .zip(right.as_str())
        .and_then(|(left, right)| compare(left, right))
        .ok_or_else(|| super::QueryError::General("invalid JSONB comparison input".into()))?;
    Ok(ScalarValue::Bool(match op {
        RowComparisonOp::Eq | RowComparisonOp::IsNotDistinctFrom => ordering.is_eq(),
        RowComparisonOp::NotEq | RowComparisonOp::IsDistinctFrom => !ordering.is_eq(),
        RowComparisonOp::Lt => ordering.is_lt(),
        RowComparisonOp::Lte => !ordering.is_gt(),
        RowComparisonOp::Gt => ordering.is_gt(),
        RowComparisonOp::Gte => !ordering.is_lt(),
    }))
}

pub(super) fn compare(left: &str, right: &str) -> Option<Ordering> {
    let left: Value = serde_json::from_str(left).ok()?;
    let right: Value = serde_json::from_str(right).ok()?;
    // PostgreSQL stores a root scalar in a flagged one-element array. Only
    // the root empty array precedes those scalar wrappers; nested arrays
    // retain the ordinary array type rank.
    let left_empty = left.as_array().is_some_and(Vec::is_empty);
    let right_empty = right.as_array().is_some_and(Vec::is_empty);
    if left_empty || right_empty {
        return Some(right_empty.cmp(&left_empty));
    }
    values(&left, &right)
}

fn values(left: &Value, right: &Value) -> Option<Ordering> {
    let class = rank(left).cmp(&rank(right));
    if !class.is_eq() {
        return Some(class);
    }
    Some(match (left, right) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Bool(left), Value::Bool(right)) => left.cmp(right),
        (Value::String(left), Value::String(right)) => left.as_bytes().cmp(right.as_bytes()),
        (Value::Number(left), Value::Number(right)) => {
            let left = crate::types::extended::numeric::parse_text(left.as_str()).ok()?;
            let right = crate::types::extended::numeric::parse_text(right.as_str()).ok()?;
            left.compare(&right)
        }
        (Value::Array(left), Value::Array(right)) => {
            let size = left.len().cmp(&right.len());
            if !size.is_eq() {
                return Some(size);
            }
            let mut order = Ordering::Equal;
            for (left, right) in left.iter().zip(right) {
                order = values(left, right)?;
                if !order.is_eq() {
                    break;
                }
            }
            order
        }
        (Value::Object(left), Value::Object(right)) => objects(left, right)?,
        _ => return None,
    })
}

fn objects(
    left: &serde_json::Map<String, Value>,
    right: &serde_json::Map<String, Value>,
) -> Option<Ordering> {
    let size = left.len().cmp(&right.len());
    if !size.is_eq() {
        return Some(size);
    }
    let left_keys = fields(left);
    let right_keys = fields(right);
    for (left_key, right_key) in left_keys.into_iter().zip(right_keys) {
        let key = left_key.as_bytes().cmp(right_key.as_bytes());
        if !key.is_eq() {
            return Some(key);
        }
        let value = values(&left[left_key], &right[right_key])?;
        if !value.is_eq() {
            return Some(value);
        }
    }
    Some(Ordering::Equal)
}

fn rank(value: &Value) -> u8 {
    match value {
        Value::Null => 1,
        Value::String(_) => 2,
        Value::Number(_) => 3,
        Value::Bool(_) => 4,
        Value::Array(_) => 5,
        Value::Object(_) => 6,
    }
}

fn fields(map: &serde_json::Map<String, Value>) -> Vec<&String> {
    let mut keys = map.keys().collect::<Vec<_>>();
    keys.sort_unstable_by(|left, right| {
        left.len()
            .cmp(&right.len())
            .then_with(|| left.as_bytes().cmp(right.as_bytes()))
    });
    keys
}
