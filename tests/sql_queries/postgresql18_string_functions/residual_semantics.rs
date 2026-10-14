//! Residual PostgreSQL 18 string edge cases tracked by #803.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_preserve_split_part_empty_field_semantics() {
    // Arrange
    let fixture = sql_fixture("issue-803-split-part-empty-null-delimiters", &[]);

    // Act
    let result = fixture.execute(
        "SELECT split_part('abc', '', 1), split_part('abc', '', 2), split_part('abc', NULL, 1), split_part('a,,b,', ',', 2), split_part('a,,b,', ',', 4)",
    );

    // Assert
    assert_eq!(
        result.expect("empty and NULL split_part delimiters").rows(),
        &[vec![
            Value::String("abc".to_owned()),
            Value::String(String::new()),
            Value::Null,
            Value::String(String::new()),
            Value::String(String::new()),
        ]]
    );
}

#[test]
fn should_apply_nonpositive_substring_offsets_by_characters() {
    // Arrange
    let fixture = sql_fixture("issue-803-unicode-substring-offsets", &[]);

    // Act
    let result =
        fixture.execute("SELECT substring('Aé🙂Z' FROM 0 FOR 4), substring('Aé🙂Z' FROM -2 FOR 4)");

    // Assert
    assert_eq!(
        result
            .expect("Unicode character-indexed substring offsets")
            .rows(),
        &[vec![
            Value::String("Aé🙂".to_owned()),
            Value::String("A".to_owned()),
        ]]
    );
}

#[test]
fn should_apply_unicode_character_offsets_to_overlay() {
    // Arrange
    let fixture = sql_fixture("issue-803-unicode-overlay-offsets", &[]);

    // Act
    let result = fixture.execute(
        "SELECT overlay('Aé🙂Z' PLACING 'x' FROM 2 FOR 1), overlay('Aé🙂Z' PLACING 'x' FROM 2 FOR 0)",
    );

    // Assert
    assert_eq!(
        result.expect("Unicode character-indexed overlay").rows(),
        &[vec![
            Value::String("Ax🙂Z".to_owned()),
            Value::String("Axé🙂Z".to_owned()),
        ]]
    );
}

#[test]
fn should_preserve_null_quote_literal_separately_from_quote_nullable() {
    // Arrange
    let fixture = sql_fixture("issue-803-quote-literal-null", &[]);

    // Act
    let result = fixture
        .execute("SELECT quote_literal(CAST(NULL AS TEXT)), quote_nullable(CAST(NULL AS TEXT))");

    // Assert
    assert_eq!(
        result.expect("NULL literal quoting forms").rows(),
        &[vec![Value::Null, Value::String("NULL".to_owned())]]
    );
}

#[test]
fn should_return_the_unicode_codepoint_from_ascii() {
    // Arrange
    let fixture = sql_fixture("issue-803-ascii-unicode-codepoint", &[]);

    // Act
    let result = fixture.execute("SELECT ascii('🙂')");

    // Assert
    assert_eq!(
        result.expect("Unicode ascii codepoint").rows(),
        &[vec![Value::Int64(128_578)]]
    );
}

#[test]
fn should_return_zero_for_empty_ascii_input() {
    // Arrange
    let fixture = sql_fixture("issue-803-ascii-empty-string", &[]);

    // Act
    let result = fixture.execute("SELECT ascii('')");

    // Assert
    assert_eq!(
        result.expect("empty ascii input").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_repeat_multicharacter_unicode_padding_patterns_by_characters() {
    // Arrange
    let fixture = sql_fixture("issue-803-multicharacter-unicode-padding", &[]);

    // Act
    let result = fixture.execute("SELECT lpad('é', 5, '🙂x'), rpad('é', 5, '🙂x')");

    // Assert
    assert_eq!(
        result.expect("multicharacter Unicode padding").rows(),
        &[vec![
            Value::String("🙂x🙂xé".to_owned()),
            Value::String("é🙂x🙂x".to_owned()),
        ]]
    );
}
