pub(super) fn equal(left: &str, right: &str) -> Option<bool> {
    let left = parse(left)?;
    let right = parse(right)?;
    Some(values_equal(left.get(), right.get()))
}

fn parse(input: &str) -> Option<crate::runtime::accounted::Accounted<serde_json::Value>> {
    crate::runtime::accounted::json_arena::parse_value_with_optional_controls(
        input.as_bytes(),
        None,
    )
    .ok()
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
