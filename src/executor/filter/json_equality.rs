use crate::executor::semantic::compare_numeric_values;
use crate::types::Value;

pub(super) fn equal(left: &str, right: &str) -> Option<bool> {
    serde_json::from_str::<serde_json::Value>(left)
        .ok()
        .zip(serde_json::from_str::<serde_json::Value>(right).ok())
        .map(|(left, right)| values_equal(&left, &right))
}

fn values_equal(left: &serde_json::Value, right: &serde_json::Value) -> bool {
    match (left, right) {
        (serde_json::Value::Number(left), serde_json::Value::Number(right)) => {
            numbers_equal(left, right)
        }
        (serde_json::Value::Array(left), serde_json::Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| values_equal(left, right))
        }
        (serde_json::Value::Object(left), serde_json::Value::Object(right)) => {
            left.len() == right.len()
                && left.iter().all(|(key, left)| {
                    right
                        .get(key)
                        .is_some_and(|right| values_equal(left, right))
                })
        }
        _ => left == right,
    }
}

fn numeric_value(number: &serde_json::Number) -> Option<Value> {
    if let Some(integer) = number.as_i64() {
        Some(Value::Int64(integer))
    } else if number.as_u64().is_some() {
        None
    } else {
        number.as_f64().map(Value::Float64)
    }
}

fn numbers_equal(left: &serde_json::Number, right: &serde_json::Number) -> bool {
    if let Some((left, right)) = numeric_value(left).zip(numeric_value(right)) {
        return compare_numeric_values(&left, &right).is_some_and(std::cmp::Ordering::is_eq);
    }
    match (left.as_u64(), right.as_u64()) {
        (Some(left), Some(right)) => left == right,
        (Some(integer), None) => unsigned_float_equal(integer, right),
        (None, Some(integer)) => unsigned_float_equal(integer, left),
        (None, None) => left == right,
    }
}

fn unsigned_float_equal(integer: u64, number: &serde_json::Number) -> bool {
    number.as_f64().is_some_and(|float| {
        float.fract() == 0.0 && format!("{float:.0}").parse::<u64>().ok() == Some(integer)
    })
}
