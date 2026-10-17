//! Native BYTEA invalid-input phases, SQLSTATE and messages.
use crate::support_bytea_input_errors as errors;

#[test]
fn should_reject_short_octal_given_cte_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\0'::BYTEA)) SELECT v FROM q";
    // Act
    let batches = errors::parse("byteainput-error-cte-short_octal", sql);
    // Assert
    errors::assert_parse_result(&batches, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_reject_long_octal_given_cte_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\400'::BYTEA)) SELECT v FROM q";
    // Act
    let batches = errors::parse("byteainput-error-cte-long_octal", sql);
    // Assert
    errors::assert_parse_result(&batches, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_reject_bad_octal_given_cte_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\378'::BYTEA)) SELECT v FROM q";
    // Act
    let batches = errors::parse("byteainput-error-cte-bad_octal", sql);
    // Assert
    errors::assert_parse_result(&batches, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_reject_bare_slash_given_cte_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\'::BYTEA)) SELECT v FROM q";
    // Act
    let batches = errors::parse("byteainput-error-cte-bare_slash", sql);
    // Assert
    errors::assert_parse_result(&batches, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_reject_upper_x_given_cte_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\X00'::BYTEA)) SELECT v FROM q";
    // Act
    let batches = errors::parse("byteainput-error-cte-upper_x", sql);
    // Assert
    errors::assert_parse_result(&batches, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_reject_mixed_prefix_given_cte_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('a\\x00'::BYTEA)) SELECT v FROM q";
    // Act
    let batches = errors::parse("byteainput-error-cte-mixed_prefix", sql);
    // Assert
    errors::assert_parse_result(&batches, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_reject_bad_hex_given_cte_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\xGG'::BYTEA)) SELECT v FROM q";
    // Act
    let batches = errors::parse("byteainput-error-cte-bad_hex", sql);
    // Assert
    errors::assert_parse_result(&batches, "22023", "invalid hexadecimal digit: \"G\"");
}

#[test]
fn should_reject_odd_hex_given_cte_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\x0'::BYTEA)) SELECT v FROM q";
    // Act
    let batches = errors::parse("byteainput-error-cte-odd_hex", sql);
    // Assert
    errors::assert_parse_result(
        &batches,
        "22023",
        "invalid hexadecimal data: odd number of digits",
    );
}

#[test]
fn should_reject_odd_spaced_given_cte_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\x 0'::BYTEA)) SELECT v FROM q";
    // Act
    let batches = errors::parse("byteainput-error-cte-odd_spaced", sql);
    // Assert
    errors::assert_parse_result(
        &batches,
        "22023",
        "invalid hexadecimal data: odd number of digits",
    );
}

#[test]
fn should_reject_split_pair_given_cte_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\x0 0'::BYTEA)) SELECT v FROM q";
    // Act
    let batches = errors::parse("byteainput-error-cte-split_pair", sql);
    // Assert
    errors::assert_parse_result(&batches, "22023", "invalid hexadecimal digit: \" \"");
}

#[test]
fn should_reject_ff_hex_given_cte_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\x\u{c}00'::BYTEA)) SELECT v FROM q";
    // Act
    let batches = errors::parse("byteainput-error-cte-ff_hex", sql);
    // Assert
    errors::assert_parse_result(&batches, "22023", "invalid hexadecimal digit: \"\u{c}\"");
}

#[test]
fn should_reject_vt_hex_given_cte_input_when_native_parse_conversion_fails() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\x\u{b}00'::BYTEA)) SELECT v FROM q";
    // Act
    let batches = errors::parse("byteainput-error-cte-vt_hex", sql);
    // Assert
    errors::assert_parse_result(&batches, "22023", "invalid hexadecimal digit: \"\u{b}\"");
}
