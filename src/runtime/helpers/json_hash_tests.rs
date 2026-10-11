use std::hash::{DefaultHasher, Hash, Hasher};

use crate::types::Value;

use super::hash_params;

#[test]
fn should_preserve_default_hasher_json_parameter_identity() {
    // Arrange
    let values = [
        serde_json::Value::Null,
        serde_json::json!({}),
        serde_json::json!([]),
        serde_json::json!({"nested": {"array": [true, null, 1, -2, 1.25]}}),
        serde_json::json!({"utf8": "é漢字🙂", "escaped": "\"\\\n\t\u{0000}"}),
        serde_json::json!([0.0, -0.0, 1.234_567_890_123_456, 1.0e100, 1.0e-100]),
        serde_json::json!([[], {}, "", ["a", "b"], {"b": 2, "a": 1}]),
    ];

    // Act
    let identities = values
        .into_iter()
        .map(|value| {
            let mut legacy = DefaultHasher::new();
            6_u8.hash(&mut legacy);
            value.to_string().hash(&mut legacy);
            let actual = hash_params(&[Value::Json(value)]);
            (actual, legacy.finish())
        })
        .collect::<Vec<_>>();

    // Assert
    for (actual, legacy) in identities {
        assert_eq!(actual, legacy);
    }
}

#[test]
fn should_preserve_json_parameter_boundaries_with_mixed_scalars() {
    // Arrange
    let first = serde_json::json!({"key": ["a", "bc"]});
    let second = serde_json::json!({"key": ["ab", "c"]});
    let mut legacy = DefaultHasher::new();
    6_u8.hash(&mut legacy);
    first.to_string().hash(&mut legacy);
    4_u8.hash(&mut legacy);
    "between".hash(&mut legacy);
    6_u8.hash(&mut legacy);
    second.to_string().hash(&mut legacy);
    2_u8.hash(&mut legacy);
    42_i64.hash(&mut legacy);
    let expected = legacy.finish();

    // Act
    let actual = hash_params(&[
        Value::Json(first),
        Value::String("between".to_owned()),
        Value::Json(second),
        Value::Int64(42),
    ]);

    // Assert
    assert_eq!(actual, expected);
}
