//! Native BYTEA odd-digit precedence and byte interpretation probes.
use crate::support_bind_planning as planning;
use crate::support_bytea_input_errors as errors;
use crate::support_values_casts;

#[test]
fn should_preserve_probe_0_given_ambiguous_bytea_input_when_native_precedence_applies() {
    // Arrange
    let sql = "SELECT '\\xG'::BYTEA AS output";
    // Act
    let batches = errors::parse("byteainput-precedence-0", sql);
    // Assert
    errors::assert_parse_result(&batches, "22023", "invalid hexadecimal digit: \"G\"");
}

#[test]
fn should_preserve_probe_1_given_ambiguous_bytea_input_when_native_precedence_applies() {
    // Arrange
    let sql = "SELECT '\\xG0'::BYTEA AS output";
    // Act
    let batches = errors::parse("byteainput-precedence-1", sql);
    // Assert
    errors::assert_parse_result(&batches, "22023", "invalid hexadecimal digit: \"G\"");
}

#[test]
fn should_preserve_probe_2_given_ambiguous_bytea_input_when_native_precedence_applies() {
    // Arrange
    let sql = "SELECT '\\x0G'::BYTEA AS output";
    // Act
    let batches = errors::parse("byteainput-precedence-2", sql);
    // Assert
    errors::assert_parse_result(&batches, "22023", "invalid hexadecimal digit: \"G\"");
}

#[test]
fn should_preserve_probe_3_given_ambiguous_bytea_input_when_native_precedence_applies() {
    // Arrange
    let sql = "SELECT '\\x0 G'::BYTEA AS output";
    // Act
    let batches = errors::parse("byteainput-precedence-3", sql);
    // Assert
    errors::assert_parse_result(&batches, "22023", "invalid hexadecimal digit: \" \"");
}

#[test]
fn should_preserve_probe_4_given_ambiguous_bytea_input_when_native_precedence_applies() {
    // Arrange
    let sql = "SELECT '\\xG '::BYTEA AS output";
    // Act
    let batches = errors::parse("byteainput-precedence-4", sql);
    // Assert
    errors::assert_parse_result(&batches, "22023", "invalid hexadecimal digit: \"G\"");
}

#[test]
fn should_preserve_probe_5_given_ambiguous_bytea_input_when_native_precedence_applies() {
    // Arrange
    let sql = "SELECT '\\x  '::BYTEA AS output";
    // Act
    let batches = planning::pruned("byteainput-precedence-5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [&[&[0, 1, 0, 0, 0, 2, 92, 120]], &[&[0, 1, 0, 0, 0, 0]]],
    );
}

#[test]
fn should_preserve_probe_6_given_ambiguous_bytea_input_when_native_precedence_applies() {
    // Arrange
    let sql = "SELECT '\\0000'::BYTEA AS output";
    // Act
    let batches = planning::pruned("byteainput-precedence-6", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 92, 120, 48, 48, 51, 48]],
            &[&[0, 1, 0, 0, 0, 2, 0, 48]],
        ],
    );
}

#[test]
fn should_preserve_probe_7_given_ambiguous_bytea_input_when_native_precedence_applies() {
    // Arrange
    let sql = "SELECT '\\3777'::BYTEA AS output";
    // Act
    let batches = planning::pruned("byteainput-precedence-7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 92, 120, 102, 102, 51, 55]],
            &[&[0, 1, 0, 0, 0, 2, 255, 55]],
        ],
    );
}

#[test]
fn should_preserve_probe_8_given_ambiguous_bytea_input_when_native_precedence_applies() {
    // Arrange
    let sql = "SELECT '\\12'::BYTEA AS output";
    // Act
    let batches = errors::parse("byteainput-precedence-8", sql);
    // Assert
    errors::assert_parse_result(&batches, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_preserve_probe_9_given_ambiguous_bytea_input_when_native_precedence_applies() {
    // Arrange
    let sql = "SELECT '\\1234'::BYTEA AS output";
    // Act
    let batches = planning::pruned("byteainput-precedence-9", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 92, 120, 53, 51, 51, 52]],
            &[&[0, 1, 0, 0, 0, 2, 83, 52]],
        ],
    );
}

#[test]
fn should_preserve_probe_10_given_ambiguous_bytea_input_when_native_precedence_applies() {
    // Arrange
    let sql = "SELECT '\\00é'::BYTEA AS output";
    // Act
    let batches = errors::parse("byteainput-precedence-10", sql);
    // Assert
    errors::assert_parse_result(&batches, "22P02", "invalid input syntax for type bytea");
}

#[test]
fn should_preserve_probe_11_given_ambiguous_bytea_input_when_native_precedence_applies() {
    // Arrange
    let sql = "SELECT 'é\\000🙂'::BYTEA AS output";
    // Act
    let batches = planning::pruned("byteainput-precedence-11", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 16, 92, 120, 99, 51, 97, 57, 48, 48, 102, 48, 57, 102, 57, 57, 56,
                50,
            ]],
            &[&[0, 1, 0, 0, 0, 7, 195, 169, 0, 240, 159, 153, 130]],
        ],
    );
}
