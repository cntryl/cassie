//! Native BYTEA invalid-input phases, SQLSTATE and messages.
use crate::support_bytea_input_errors as errors;

#[test]
fn should_reject_short_octal_given_dead_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT CASE WHEN FALSE THEN '\\0'::BYTEA ELSE NULL::BYTEA END";
    // Act
    let batches = errors::parse("byteainput-error-dead-short_octal", sql);
    // Assert
    errors::assert_parse_result(&batches, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_reject_long_octal_given_dead_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT CASE WHEN FALSE THEN '\\400'::BYTEA ELSE NULL::BYTEA END";
    // Act
    let batches = errors::parse("byteainput-error-dead-long_octal", sql);
    // Assert
    errors::assert_parse_result(&batches, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_reject_bad_octal_given_dead_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT CASE WHEN FALSE THEN '\\378'::BYTEA ELSE NULL::BYTEA END";
    // Act
    let batches = errors::parse("byteainput-error-dead-bad_octal", sql);
    // Assert
    errors::assert_parse_result(&batches, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_reject_bare_slash_given_dead_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT CASE WHEN FALSE THEN '\\'::BYTEA ELSE NULL::BYTEA END";
    // Act
    let batches = errors::parse("byteainput-error-dead-bare_slash", sql);
    // Assert
    errors::assert_parse_result(&batches, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_reject_upper_x_given_dead_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT CASE WHEN FALSE THEN '\\X00'::BYTEA ELSE NULL::BYTEA END";
    // Act
    let batches = errors::parse("byteainput-error-dead-upper_x", sql);
    // Assert
    errors::assert_parse_result(&batches, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_reject_mixed_prefix_given_dead_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT CASE WHEN FALSE THEN 'a\\x00'::BYTEA ELSE NULL::BYTEA END";
    // Act
    let batches = errors::parse("byteainput-error-dead-mixed_prefix", sql);
    // Assert
    errors::assert_parse_result(&batches, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_reject_bad_hex_given_dead_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT CASE WHEN FALSE THEN '\\xGG'::BYTEA ELSE NULL::BYTEA END";
    // Act
    let batches = errors::parse("byteainput-error-dead-bad_hex", sql);
    // Assert
    errors::assert_parse_result(&batches, "22023", "invalid hexadecimal digit: \"G\"");
}

#[test]
fn should_reject_odd_hex_given_dead_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT CASE WHEN FALSE THEN '\\x0'::BYTEA ELSE NULL::BYTEA END";
    // Act
    let batches = errors::parse("byteainput-error-dead-odd_hex", sql);
    // Assert
    errors::assert_parse_result(
        &batches,
        "22023",
        "invalid hexadecimal data: odd number of digits",
    );
}

#[test]
fn should_reject_odd_spaced_given_dead_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT CASE WHEN FALSE THEN '\\x 0'::BYTEA ELSE NULL::BYTEA END";
    // Act
    let batches = errors::parse("byteainput-error-dead-odd_spaced", sql);
    // Assert
    errors::assert_parse_result(
        &batches,
        "22023",
        "invalid hexadecimal data: odd number of digits",
    );
}

#[test]
fn should_reject_split_pair_given_dead_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT CASE WHEN FALSE THEN '\\x0 0'::BYTEA ELSE NULL::BYTEA END";
    // Act
    let batches = errors::parse("byteainput-error-dead-split_pair", sql);
    // Assert
    errors::assert_parse_result(&batches, "22023", "invalid hexadecimal digit: \" \"");
}

#[test]
fn should_reject_ff_hex_given_dead_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT CASE WHEN FALSE THEN '\\x\u{c}00'::BYTEA ELSE NULL::BYTEA END";
    // Act
    let batches = errors::parse("byteainput-error-dead-ff_hex", sql);
    // Assert
    errors::assert_parse_result(&batches, "22023", "invalid hexadecimal digit: \"\u{c}\"");
}

#[test]
fn should_reject_vt_hex_given_dead_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "SELECT CASE WHEN FALSE THEN '\\x\u{b}00'::BYTEA ELSE NULL::BYTEA END";
    // Act
    let batches = errors::parse("byteainput-error-dead-vt_hex", sql);
    // Assert
    errors::assert_parse_result(&batches, "22023", "invalid hexadecimal digit: \"\u{b}\"");
}
