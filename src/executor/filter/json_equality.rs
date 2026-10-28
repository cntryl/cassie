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

fn numbers_equal(left: &serde_json::Number, right: &serde_json::Number) -> bool {
    let left_number = crate::types::extended::numeric::parse_text(left.as_str());
    let right_number = crate::types::extended::numeric::parse_text(right.as_str());
    match (left_number, right_number) {
        (Ok(left), Ok(right)) => left.compare(&right).is_eq(),
        _ => left == right,
    }
}
