//! Compare actual carriers with independently recorded literal rows.
use cassie::types::Value;

fn matches_literal(actual: &Value, expected: &serde_json::Value) -> bool {
    match actual {
        Value::Null => expected.is_null(),
        Value::Bool(value) => expected.as_bool() == Some(*value),
        Value::Int64(value) => expected.as_i64() == Some(*value),
        Value::Float64(value) => expected.as_f64() == Some(*value),
        Value::String(value) => expected.as_str() == Some(value.as_str()),
        Value::Json(value) => value == expected,
        Value::Vector(_) => false,
    }
}

pub fn matches_row(actual: &[Value], expected: &[serde_json::Value]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| matches_literal(actual, expected))
}
