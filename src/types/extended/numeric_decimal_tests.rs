use super::{parse_text, MAX_DSCALE, MAX_INTEGER_DIGITS};

#[test]
fn should_accept_numeric_integer_weight_after_ignoring_coefficient_leading_zeroes() {
    // Arrange
    let cases = ["0.01e131073", "0000.01e131073", "-0.01e131073"];
    // Act
    let values = cases.map(|text| {
        parse_text(text)
            .expect("pinned numeric boundary")
            .to_string()
    });
    // Assert
    assert_eq!(values[0].len(), MAX_INTEGER_DIGITS);
    assert_eq!(values[1], values[0]);
    assert_eq!(values[2].len(), MAX_INTEGER_DIGITS + 1);
    assert!(values[0].starts_with('1'));
    assert!(values[2].starts_with("-1"));
}

#[test]
fn should_reject_true_numeric_weight_overflow_after_significant_digit_counting() {
    // Arrange
    let cases = ["0.01e131074", "0000.01e131074", "-0.01e131074"];
    // Act
    let errors = cases.map(|text| parse_text(text).expect_err("one digit above format"));
    // Assert
    assert!(errors.iter().all(|error| error.sqlstate == "22003"));
}

#[test]
fn should_preserve_numeric_zero_format_bounds_before_dense_coefficient_construction() {
    // Arrange
    let cases = ["0e1073741823", "0.00e-16381", "-0.00e-16381"];
    // Act
    let values = cases.map(|text| parse_text(text).expect("zero boundary").to_string());
    let errors = ["0e1073741824", "0.00e-16382"]
        .map(|text| parse_text(text).expect_err("zero format overflow"));
    // Assert
    assert_eq!(values[0], "0");
    assert_eq!(
        values[1].len(),
        usize::try_from(MAX_DSCALE).expect("scale") + 2
    );
    assert_eq!(values[1], values[2]);
    assert!(errors.iter().all(|error| error.sqlstate == "22003"));
}

#[test]
fn should_preserve_numeric_syntax_errors_ahead_of_format_range_checks() {
    // Arrange
    let cases = ["1e131072x", "0.01e131073x", "1e+", "1.2.3e131074"];
    // Act
    let errors = cases.map(|text| parse_text(text).expect_err("invalid complete numeric grammar"));
    // Assert
    assert!(errors.iter().all(|error| error.sqlstate == "22P02"));
}

use crate::types::jsonb_numeric_cases as fixture;

#[test]
fn should_preserve_pinned_numeric_display_scale_across_decimal_weight_regimes() {
    // Arrange
    let cases = fixture::CASES;
    // Act
    let values = cases
        .iter()
        .map(|(input, _)| parse_text(input).expect("exact numeric").to_string())
        .collect::<Vec<_>>();
    // Assert
    for ((input, expected), actual) in cases.iter().zip(values) {
        assert_eq!(&actual, expected, "{input}");
    }
}
