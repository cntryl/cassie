//! Native malformed BYTEA text parameters recover on the same statement.
use crate::support_bytea_input_errors as errors;

#[test]
fn should_recover_short_octal_given_oid_17_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\0".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-17-short_octal", input, 17);
    // Assert
    errors::assert_parameter_result(&batches, 17, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_recover_short_octal_given_oid_25_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\0".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-25-short_octal", input, 25);
    // Assert
    errors::assert_parameter_result(&batches, 25, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_recover_long_octal_given_oid_17_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\400".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-17-long_octal", input, 17);
    // Assert
    errors::assert_parameter_result(&batches, 17, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_recover_long_octal_given_oid_25_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\400".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-25-long_octal", input, 25);
    // Assert
    errors::assert_parameter_result(&batches, 25, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_recover_bad_octal_given_oid_17_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\378".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-17-bad_octal", input, 17);
    // Assert
    errors::assert_parameter_result(&batches, 17, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_recover_bad_octal_given_oid_25_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\378".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-25-bad_octal", input, 25);
    // Assert
    errors::assert_parameter_result(&batches, 25, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_recover_bare_slash_given_oid_17_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-17-bare_slash", input, 17);
    // Assert
    errors::assert_parameter_result(&batches, 17, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_recover_bare_slash_given_oid_25_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-25-bare_slash", input, 25);
    // Assert
    errors::assert_parameter_result(&batches, 25, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_recover_upper_x_given_oid_17_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\X00".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-17-upper_x", input, 17);
    // Assert
    errors::assert_parameter_result(&batches, 17, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_recover_upper_x_given_oid_25_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\X00".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-25-upper_x", input, 25);
    // Assert
    errors::assert_parameter_result(&batches, 25, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_recover_mixed_prefix_given_oid_17_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "a\\x00".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-17-mixed_prefix", input, 17);
    // Assert
    errors::assert_parameter_result(&batches, 17, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_recover_mixed_prefix_given_oid_25_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "a\\x00".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-25-mixed_prefix", input, 25);
    // Assert
    errors::assert_parameter_result(&batches, 25, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_recover_bad_hex_given_oid_17_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\xGG".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-17-bad_hex", input, 17);
    // Assert
    errors::assert_parameter_result(&batches, 17, "22023", "invalid hexadecimal digit: \"G\"");
}

#[test]
fn should_recover_bad_hex_given_oid_25_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\xGG".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-25-bad_hex", input, 25);
    // Assert
    errors::assert_parameter_result(&batches, 25, "22023", "invalid hexadecimal digit: \"G\"");
}

#[test]
fn should_recover_odd_hex_given_oid_17_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\x0".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-17-odd_hex", input, 17);
    // Assert
    errors::assert_parameter_result(
        &batches,
        17,
        "22023",
        "invalid hexadecimal data: odd number of digits",
    );
}

#[test]
fn should_recover_odd_hex_given_oid_25_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\x0".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-25-odd_hex", input, 25);
    // Assert
    errors::assert_parameter_result(
        &batches,
        25,
        "22023",
        "invalid hexadecimal data: odd number of digits",
    );
}

#[test]
fn should_recover_odd_spaced_given_oid_17_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\x 0".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-17-odd_spaced", input, 17);
    // Assert
    errors::assert_parameter_result(
        &batches,
        17,
        "22023",
        "invalid hexadecimal data: odd number of digits",
    );
}

#[test]
fn should_recover_odd_spaced_given_oid_25_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\x 0".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-25-odd_spaced", input, 25);
    // Assert
    errors::assert_parameter_result(
        &batches,
        25,
        "22023",
        "invalid hexadecimal data: odd number of digits",
    );
}

#[test]
fn should_recover_split_pair_given_oid_17_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\x0 0".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-17-split_pair", input, 17);
    // Assert
    errors::assert_parameter_result(&batches, 17, "22023", "invalid hexadecimal digit: \" \"");
}

#[test]
fn should_recover_split_pair_given_oid_25_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\x0 0".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-25-split_pair", input, 25);
    // Assert
    errors::assert_parameter_result(&batches, 25, "22023", "invalid hexadecimal digit: \" \"");
}

#[test]
fn should_recover_ff_hex_given_oid_17_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\x\u{c}00".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-17-ff_hex", input, 17);
    // Assert
    errors::assert_parameter_result(
        &batches,
        17,
        "22023",
        "invalid hexadecimal digit: \"\u{c}\"",
    );
}

#[test]
fn should_recover_ff_hex_given_oid_25_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\x\u{c}00".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-25-ff_hex", input, 25);
    // Assert
    errors::assert_parameter_result(
        &batches,
        25,
        "22023",
        "invalid hexadecimal digit: \"\u{c}\"",
    );
}

#[test]
fn should_recover_vt_hex_given_oid_17_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\x\u{b}00".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-17-vt_hex", input, 17);
    // Assert
    errors::assert_parameter_result(
        &batches,
        17,
        "22023",
        "invalid hexadecimal digit: \"\u{b}\"",
    );
}

#[test]
fn should_recover_vt_hex_given_oid_25_text_input_when_bind_rejects_bytea() {
    // Arrange
    let input = "\\x\u{b}00".as_bytes();
    // Act
    let batches = errors::parameter("byteainput-param-error-25-vt_hex", input, 25);
    // Assert
    errors::assert_parameter_result(
        &batches,
        25,
        "22023",
        "invalid hexadecimal digit: \"\u{b}\"",
    );
}
