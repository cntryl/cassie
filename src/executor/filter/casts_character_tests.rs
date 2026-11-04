//! Same-family CHAR coercion preserves stored value widths before presentation.
use super::*;

#[test]
fn should_preserve_bpchar_bytes_given_an_unconstrained_same_family_coercion() {
    // Arrange
    let input = ScalarValue::Str("a  ".into());
    let source = DataType::Char { length: Some(3) };

    // Act
    let result = scalar(
        &input,
        &DataType::Char { length: None },
        Some(&source),
        None,
    );

    // Assert
    assert!(matches!(result, Ok(ScalarValue::Str(text)) if text == "a  "));
}

#[test]
fn should_trim_bpchar_blanks_given_an_explicit_text_target() {
    // Arrange
    let input = ScalarValue::Str("a  ".into());
    let source = DataType::Char { length: Some(3) };

    // Act
    let result = scalar(&input, &DataType::Text, Some(&source), None);

    // Assert
    assert!(matches!(result, Ok(ScalarValue::Str(text)) if text == "a"));
}
