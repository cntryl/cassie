//! Character padding must preserve comparison and distinct semantics.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_same_value_widths_given_character_padding_when_semantic_keys_are_compared() {
    // Arrange
    let sql = "SELECT 'a'::CHAR(3)='a'::CHAR(5) AS result";
    // Act
    let batches = planning::pruned("valuescasts-semantics-same_value_widths", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("result", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_preserve_unknown_spaces_given_character_padding_when_semantic_keys_are_compared() {
    // Arrange
    let sql = "SELECT 'a'::CHAR(3)='a ' AS result";
    // Act
    let batches = planning::pruned("valuescasts-semantics-unknown_spaces", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("result", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_preserve_typed_text_spaces_given_character_padding_when_semantic_keys_are_compared() {
    // Arrange
    let sql = "SELECT 'a'::CHAR(3)='a '::TEXT AS result";
    // Act
    let batches = planning::pruned("valuescasts-semantics-typed_text_spaces", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("result", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_preserve_typed_varchar_spaces_given_character_padding_when_semantic_keys_are_compared() {
    // Arrange
    let sql = "SELECT 'a'::CHAR(3)='a '::VARCHAR(5) AS result";
    // Act
    let batches = planning::pruned("valuescasts-semantics-typed_varchar_spaces", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("result", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_preserve_not_distinct_widths_given_character_padding_when_semantic_keys_are_compared() {
    // Arrange
    let sql = "SELECT 'a'::CHAR(3) IS NOT DISTINCT FROM 'a'::CHAR(5) AS result";
    // Act
    let batches = planning::pruned("valuescasts-semantics-not_distinct_widths", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("result", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_preserve_ordered_widths_given_character_padding_when_semantic_keys_are_compared() {
    // Arrange
    let sql = "SELECT 'a'::CHAR(3)<'a'::CHAR(5) AS result";
    // Act
    let batches = planning::pruned("valuescasts-semantics-ordered_widths", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("result", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_preserve_values_equality_given_character_padding_when_semantic_keys_are_compared() {
    // Arrange
    let sql = "SELECT v='a' AS result FROM (VALUES ('a'::CHAR(3)),('a'::CHAR(5))) q(v)";
    // Act
    let batches = planning::pruned("valuescasts-semantics-values_equality", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("result", 16, 1, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 116], &[0, 1, 0, 0, 0, 1, 116]],
            &[&[0, 1, 0, 0, 0, 1, 1], &[0, 1, 0, 0, 0, 1, 1]],
        ],
    );
}

#[test]
fn should_preserve_values_distinct_given_character_padding_when_semantic_keys_are_compared() {
    // Arrange
    let sql = "SELECT COUNT(*)::INT AS result FROM (SELECT DISTINCT v FROM (VALUES ('a'::CHAR(3)),('a'::CHAR(5))) q(v)) d";
    // Act
    let batches = planning::pruned("valuescasts-semantics-values_distinct", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("result", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1]],
        ],
    );
}
