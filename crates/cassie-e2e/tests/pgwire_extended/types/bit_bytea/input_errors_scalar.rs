//! Native BYTEA invalid-input phases, SQLSTATE and messages.
use crate::support_bytea_input_errors as errors;

#[test]
fn should_reject_short_octal_given_scalar_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT '\\0'::BYTEA";
    // Act
    let batches = errors::parse("byteainput-error-scalar-short_octal", sql);
    // Assert
    errors::assert_parse_result(&batches, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_reject_long_octal_given_scalar_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT '\\400'::BYTEA";
    // Act
    let batches = errors::parse("byteainput-error-scalar-long_octal", sql);
    // Assert
    errors::assert_parse_result(&batches, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_reject_bad_octal_given_scalar_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT '\\378'::BYTEA";
    // Act
    let batches = errors::parse("byteainput-error-scalar-bad_octal", sql);
    // Assert
    errors::assert_parse_result(&batches, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_reject_bare_slash_given_scalar_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT '\\'::BYTEA";
    // Act
    let batches = errors::parse("byteainput-error-scalar-bare_slash", sql);
    // Assert
    errors::assert_parse_result(&batches, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_reject_upper_x_given_scalar_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT '\\X00'::BYTEA";
    // Act
    let batches = errors::parse("byteainput-error-scalar-upper_x", sql);
    // Assert
    errors::assert_parse_result(&batches, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_reject_mixed_prefix_given_scalar_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT 'a\\x00'::BYTEA";
    // Act
    let batches = errors::parse("byteainput-error-scalar-mixed_prefix", sql);
    // Assert
    errors::assert_parse_result(&batches, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_reject_bad_hex_given_scalar_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT '\\xGG'::BYTEA";
    // Act
    let batches = errors::parse("byteainput-error-scalar-bad_hex", sql);
    // Assert
    errors::assert_parse_result(&batches, "22023", "invalid hexadecimal digit: \"G\"");
}

#[test]
fn should_reject_odd_hex_given_scalar_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT '\\x0'::BYTEA";
    // Act
    let batches = errors::parse("byteainput-error-scalar-odd_hex", sql);
    // Assert
    errors::assert_parse_result(
        &batches,
        "22023",
        "invalid hexadecimal data: odd number of digits",
    );
}

#[test]
fn should_reject_odd_spaced_given_scalar_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT '\\x 0'::BYTEA";
    // Act
    let batches = errors::parse("byteainput-error-scalar-odd_spaced", sql);
    // Assert
    errors::assert_parse_result(
        &batches,
        "22023",
        "invalid hexadecimal data: odd number of digits",
    );
}

#[test]
fn should_reject_split_pair_given_scalar_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT '\\x0 0'::BYTEA";
    // Act
    let batches = errors::parse("byteainput-error-scalar-split_pair", sql);
    // Assert
    errors::assert_parse_result(&batches, "22023", "invalid hexadecimal digit: \" \"");
}

#[test]
fn should_reject_ff_hex_given_scalar_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT '\\x\u{c}00'::BYTEA";
    // Act
    let batches = errors::parse("byteainput-error-scalar-ff_hex", sql);
    // Assert
    errors::assert_parse_result(&batches, "22023", "invalid hexadecimal digit: \"\u{c}\"");
}

#[test]
fn should_reject_vt_hex_given_scalar_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT '\\x\u{b}00'::BYTEA";
    // Act
    let batches = errors::parse("byteainput-error-scalar-vt_hex", sql);
    // Assert
    errors::assert_parse_result(&batches, "22023", "invalid hexadecimal digit: \"\u{b}\"");
}
