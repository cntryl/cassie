use super::{delete_operator, QueryError, ScalarValue};

#[test]
fn should_treat_first_private_number_key_as_an_ordinary_deletion_key() {
    // Arrange
    let source = ScalarValue::Json(r#"{"$serde_json::private::Number":"12","drop":0}"#.into());
    let key = ScalarValue::Str("drop".into());
    // Act
    let result = delete_operator(&source, &key).expect("deletion");
    // Assert
    assert!(
        matches!(result, ScalarValue::Json(text) if text == r#"{"$serde_json::private::Number": "12"}"#)
    );
}

#[test]
fn should_treat_first_private_raw_value_key_as_an_ordinary_deletion_key() {
    // Arrange
    let source = ScalarValue::Json(
        r#"{"$serde_json::private::RawValue":"not a JSON document","drop":0}"#.into(),
    );
    let key = ScalarValue::Str("drop".into());
    // Act
    let result = delete_operator(&source, &key).expect("deletion");
    // Assert
    assert!(
        matches!(result, ScalarValue::Json(text) if text == r#"{"$serde_json::private::RawValue": "not a JSON document"}"#)
    );
}

#[test]
fn should_keep_the_last_duplicate_before_deleting_a_different_key() {
    // Arrange
    let source = ScalarValue::Json(r#"{"a":1,"a":2,"drop":0}"#.into());
    let key = ScalarValue::Str("drop".into());
    // Act
    let result = delete_operator(&source, &key).expect("deletion");
    // Assert
    assert!(matches!(result, ScalarValue::Json(text) if text == r#"{"a": 2}"#));
}

#[test]
fn should_preserve_escaped_nul_in_a_surviving_json_deletion_field() {
    // Arrange
    let source = ScalarValue::Json(r#"{"a":"\u0000","drop":1}"#.into());
    let key = ScalarValue::Str("drop".into());
    // Act
    let result = delete_operator(&source, &key).expect("deletion");
    // Assert
    assert!(matches!(result, ScalarValue::Json(text) if text == r#"{"a": "\u0000"}"#));
}

#[test]
fn should_only_format_a_numeric_overflow_if_the_field_survives_deletion() {
    // Arrange
    let source = ScalarValue::Json(r#"{"n":1e131072,"keep":1}"#.into());
    // Act
    let removed = delete_operator(&source, &ScalarValue::Str("n".into()));
    let retained = delete_operator(&source, &ScalarValue::Str("keep".into()));
    // Assert
    assert!(matches!(removed, Ok(ScalarValue::Json(text)) if text == r#"{"keep": 1}"#));
    assert!(matches!(retained, Err(QueryError::General(_))));
}

#[test]
fn should_accept_a_deleted_subtree_deeper_than_the_value_parser_limit() {
    // Arrange
    let nested = format!("{}0{}", "[".repeat(160), "]".repeat(160));
    let source = ScalarValue::Json(format!(r#"{{"drop":{nested},"keep":1}}"#));
    // Act
    let result = delete_operator(&source, &ScalarValue::Str("drop".into()));
    // Assert
    assert!(matches!(result, Ok(ScalarValue::Json(text)) if text == r#"{"keep": 1}"#));
}

#[test]
fn should_only_decode_a_lone_surrogate_if_its_field_survives_deletion() {
    // Arrange
    let source = ScalarValue::Json(r#"{"drop":"\ud800","keep":1}"#.into());
    // Act
    let removed = delete_operator(&source, &ScalarValue::Str("drop".into()));
    let retained = delete_operator(&source, &ScalarValue::Str("keep".into()));
    // Assert
    assert!(matches!(removed, Ok(ScalarValue::Json(text)) if text == r#"{"keep": 1}"#));
    assert!(matches!(retained, Err(QueryError::General(_))));
}

#[test]
fn should_decode_keys_of_reached_objects_but_not_removed_subtrees() {
    // Arrange
    let reached = ScalarValue::Json(r#"{"\ud800":0,"drop":1}"#.into());
    let removed_subtree = ScalarValue::Json(r#"{"drop":{"\ud800":0},"keep":1}"#.into());
    let key = ScalarValue::Str("drop".into());
    // Act
    let reached_result = delete_operator(&reached, &key);
    let removed_result = delete_operator(&removed_subtree, &key);
    // Assert
    assert!(matches!(reached_result, Err(QueryError::General(_))));
    assert!(matches!(removed_result, Ok(ScalarValue::Json(text)) if text == r#"{"keep": 1}"#));
}
