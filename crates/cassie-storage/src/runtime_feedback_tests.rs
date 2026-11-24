use super::{stable_fingerprint, RuntimeFeedbackKey};

#[test]
fn should_preserve_the_persisted_feedback_fingerprint_for_a_legacy_key() {
    // Arrange
    let legacy = br#"{"schema_epoch":7,"database":"analytics","collection":"orders","operator_family":"scan","relation_set":["orders"],"predicate_shape_hash":19,"index_shape_hash":null}"#;
    let key: RuntimeFeedbackKey = serde_json::from_slice(legacy).expect("legacy feedback key");

    // Act
    let actual = stable_fingerprint(&key);

    // Assert
    assert!(key.search_path.is_empty());
    assert_eq!(actual, 0x0e2c_6951_1c09_398b);
}
