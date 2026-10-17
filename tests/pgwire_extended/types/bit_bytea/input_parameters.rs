//! Native BYTEA input parameters across source and output formats.
use crate::support_values_casts;

#[test]
fn should_decode_empty_given_oid_17_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters("byteainput-param-17-empty", sql, 17, &[], &[]);
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [&[&[0, 1, 0, 0, 0, 2, 92, 120]], &[&[0, 1, 0, 0, 0, 0]]],
    );
}

#[test]
fn should_decode_empty_given_oid_25_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters("byteainput-param-25-empty", sql, 25, &[], &[]);
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [&[&[0, 1, 0, 0, 0, 2, 92, 120]], &[&[0, 1, 0, 0, 0, 0]]],
    );
}

#[test]
fn should_decode_ascii_given_oid_17_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-17-ascii",
        sql,
        17,
        &[97, 98, 99],
        &[97, 98, 99],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 54, 49, 54, 50, 54, 51]],
            &[&[0, 1, 0, 0, 0, 3, 97, 98, 99]],
        ],
    );
}

#[test]
fn should_decode_ascii_given_oid_25_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-25-ascii",
        sql,
        25,
        &[97, 98, 99],
        &[97, 98, 99],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 54, 49, 54, 50, 54, 51]],
            &[&[0, 1, 0, 0, 0, 3, 97, 98, 99]],
        ],
    );
}

#[test]
fn should_decode_utf8_given_oid_17_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-17-utf8",
        sql,
        17,
        &[195, 169, 240, 159, 153, 130],
        &[195, 169, 240, 159, 153, 130],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 14, 92, 120, 99, 51, 97, 57, 102, 48, 57, 102, 57, 57, 56, 50,
            ]],
            &[&[0, 1, 0, 0, 0, 6, 195, 169, 240, 159, 153, 130]],
        ],
    );
}

#[test]
fn should_decode_utf8_given_oid_25_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-25-utf8",
        sql,
        25,
        &[195, 169, 240, 159, 153, 130],
        &[195, 169, 240, 159, 153, 130],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 14, 92, 120, 99, 51, 97, 57, 102, 48, 57, 102, 57, 57, 56, 50,
            ]],
            &[&[0, 1, 0, 0, 0, 6, 195, 169, 240, 159, 153, 130]],
        ],
    );
}

#[test]
fn should_decode_nul_given_oid_17_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-17-nul",
        sql,
        17,
        &[92, 48, 48, 48],
        &[0],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 92, 120, 48, 48]],
            &[&[0, 1, 0, 0, 0, 1, 0]],
        ],
    );
}

#[test]
fn should_decode_nul_given_oid_25_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-25-nul",
        sql,
        25,
        &[92, 48, 48, 48],
        &[92, 48, 48, 48],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 92, 120, 48, 48]],
            &[&[0, 1, 0, 0, 0, 1, 0]],
        ],
    );
}

#[test]
fn should_decode_high_given_oid_17_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-17-high",
        sql,
        17,
        &[92, 51, 55, 55],
        &[255],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 92, 120, 102, 102]],
            &[&[0, 1, 0, 0, 0, 1, 255]],
        ],
    );
}

#[test]
fn should_decode_high_given_oid_25_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-25-high",
        sql,
        25,
        &[92, 51, 55, 55],
        &[92, 51, 55, 55],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 92, 120, 102, 102]],
            &[&[0, 1, 0, 0, 0, 1, 255]],
        ],
    );
}

#[test]
fn should_decode_slash_given_oid_17_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches =
        support_values_casts::parameters("byteainput-param-17-slash", sql, 17, &[92, 92], &[92]);
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 92, 120, 53, 99]],
            &[&[0, 1, 0, 0, 0, 1, 92]],
        ],
    );
}

#[test]
fn should_decode_slash_given_oid_25_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-25-slash",
        sql,
        25,
        &[92, 92],
        &[92, 92],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 92, 120, 53, 99]],
            &[&[0, 1, 0, 0, 0, 1, 92]],
        ],
    );
}

#[test]
fn should_decode_mixed_given_oid_17_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-17-mixed",
        sql,
        17,
        &[97, 92, 48, 48, 48, 98, 92, 92, 99, 92, 51, 55, 55],
        &[97, 0, 98, 92, 99, 255],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 14, 92, 120, 54, 49, 48, 48, 54, 50, 53, 99, 54, 51, 102, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 6, 97, 0, 98, 92, 99, 255]],
        ],
    );
}

#[test]
fn should_decode_mixed_given_oid_25_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-25-mixed",
        sql,
        25,
        &[97, 92, 48, 48, 48, 98, 92, 92, 99, 92, 51, 55, 55],
        &[97, 92, 48, 48, 48, 98, 92, 92, 99, 92, 51, 55, 55],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 14, 92, 120, 54, 49, 48, 48, 54, 50, 53, 99, 54, 51, 102, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 6, 97, 0, 98, 92, 99, 255]],
        ],
    );
}

#[test]
fn should_decode_octal_ascii_given_oid_17_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-17-octal_ascii",
        sql,
        17,
        &[92, 49, 48, 49, 92, 49, 48, 50, 92, 49, 48, 51],
        &[65, 66, 67],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 52, 49, 52, 50, 52, 51]],
            &[&[0, 1, 0, 0, 0, 3, 65, 66, 67]],
        ],
    );
}

#[test]
fn should_decode_octal_ascii_given_oid_25_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-25-octal_ascii",
        sql,
        25,
        &[92, 49, 48, 49, 92, 49, 48, 50, 92, 49, 48, 51],
        &[92, 49, 48, 49, 92, 49, 48, 50, 92, 49, 48, 51],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 52, 49, 52, 50, 52, 51]],
            &[&[0, 1, 0, 0, 0, 3, 65, 66, 67]],
        ],
    );
}

#[test]
fn should_decode_octal_low_given_oid_17_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-17-octal_low",
        sql,
        17,
        &[
            92, 48, 48, 49, 92, 48, 48, 55, 92, 48, 49, 48, 92, 48, 49, 49, 92, 48, 49, 50, 92, 48,
            49, 53, 92, 48, 51, 55, 92, 49, 55, 55,
        ],
        &[1, 7, 8, 9, 10, 13, 31, 127],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 18, 92, 120, 48, 49, 48, 55, 48, 56, 48, 57, 48, 97, 48, 100, 49,
                102, 55, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 8, 1, 7, 8, 9, 10, 13, 31, 127]],
        ],
    );
}

#[test]
fn should_decode_octal_low_given_oid_25_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-25-octal_low",
        sql,
        25,
        &[
            92, 48, 48, 49, 92, 48, 48, 55, 92, 48, 49, 48, 92, 48, 49, 49, 92, 48, 49, 50, 92, 48,
            49, 53, 92, 48, 51, 55, 92, 49, 55, 55,
        ],
        &[
            92, 48, 48, 49, 92, 48, 48, 55, 92, 48, 49, 48, 92, 48, 49, 49, 92, 48, 49, 50, 92, 48,
            49, 53, 92, 48, 51, 55, 92, 49, 55, 55,
        ],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 18, 92, 120, 48, 49, 48, 55, 48, 56, 48, 57, 48, 97, 48, 100, 49,
                102, 55, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 8, 1, 7, 8, 9, 10, 13, 31, 127]],
        ],
    );
}

#[test]
fn should_decode_hex_empty_given_oid_17_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches =
        support_values_casts::parameters("byteainput-param-17-hex_empty", sql, 17, &[92, 120], &[]);
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [&[&[0, 1, 0, 0, 0, 2, 92, 120]], &[&[0, 1, 0, 0, 0, 0]]],
    );
}

#[test]
fn should_decode_hex_empty_given_oid_25_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-25-hex_empty",
        sql,
        25,
        &[92, 120],
        &[92, 120],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [&[&[0, 1, 0, 0, 0, 2, 92, 120]], &[&[0, 1, 0, 0, 0, 0]]],
    );
}

#[test]
fn should_decode_hex_case_given_oid_17_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-17-hex_case",
        sql,
        17,
        &[92, 120, 48, 48, 65, 66, 102, 102],
        &[0, 171, 255],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102]],
            &[&[0, 1, 0, 0, 0, 3, 0, 171, 255]],
        ],
    );
}

#[test]
fn should_decode_hex_case_given_oid_25_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-25-hex_case",
        sql,
        25,
        &[92, 120, 48, 48, 65, 66, 102, 102],
        &[92, 120, 48, 48, 65, 66, 102, 102],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102]],
            &[&[0, 1, 0, 0, 0, 3, 0, 171, 255]],
        ],
    );
}

#[test]
fn should_decode_hex_spaces_given_oid_17_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-17-hex_spaces",
        sql,
        17,
        &[92, 120, 32, 48, 48, 32, 65, 66, 32, 102, 102, 32],
        &[0, 171, 255],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102]],
            &[&[0, 1, 0, 0, 0, 3, 0, 171, 255]],
        ],
    );
}

#[test]
fn should_decode_hex_spaces_given_oid_25_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-25-hex_spaces",
        sql,
        25,
        &[92, 120, 32, 48, 48, 32, 65, 66, 32, 102, 102, 32],
        &[92, 120, 32, 48, 48, 32, 65, 66, 32, 102, 102, 32],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102]],
            &[&[0, 1, 0, 0, 0, 3, 0, 171, 255]],
        ],
    );
}

#[test]
fn should_decode_hex_lines_given_oid_17_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-17-hex_lines",
        sql,
        17,
        &[92, 120, 48, 48, 10, 49, 49, 13, 50, 50, 9, 51, 51],
        &[0, 17, 34, 51],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 49, 49, 50, 50, 51, 51]],
            &[&[0, 1, 0, 0, 0, 4, 0, 17, 34, 51]],
        ],
    );
}

#[test]
fn should_decode_hex_lines_given_oid_25_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-25-hex_lines",
        sql,
        25,
        &[92, 120, 48, 48, 10, 49, 49, 13, 50, 50, 9, 51, 51],
        &[92, 120, 48, 48, 10, 49, 49, 13, 50, 50, 9, 51, 51],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 49, 49, 50, 50, 51, 51]],
            &[&[0, 1, 0, 0, 0, 4, 0, 17, 34, 51]],
        ],
    );
}

#[test]
fn should_decode_slash_tail_given_oid_17_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-17-slash_tail",
        sql,
        17,
        &[97, 98, 99, 92, 92],
        &[97, 98, 99, 92],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 54, 49, 54, 50, 54, 51, 53, 99]],
            &[&[0, 1, 0, 0, 0, 4, 97, 98, 99, 92]],
        ],
    );
}

#[test]
fn should_decode_slash_tail_given_oid_25_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-25-slash_tail",
        sql,
        25,
        &[97, 98, 99, 92, 92],
        &[97, 98, 99, 92, 92],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 54, 49, 54, 50, 54, 51, 53, 99]],
            &[&[0, 1, 0, 0, 0, 4, 97, 98, 99, 92]],
        ],
    );
}

#[test]
fn should_decode_quote_given_oid_17_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-17-quote",
        sql,
        17,
        &[97, 39, 98],
        &[97, 39, 98],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 54, 49, 50, 55, 54, 50]],
            &[&[0, 1, 0, 0, 0, 3, 97, 39, 98]],
        ],
    );
}

#[test]
fn should_decode_quote_given_oid_25_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-25-quote",
        sql,
        25,
        &[97, 39, 98],
        &[97, 39, 98],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 54, 49, 50, 55, 54, 50]],
            &[&[0, 1, 0, 0, 0, 3, 97, 39, 98]],
        ],
    );
}

#[test]
fn should_decode_spaces_given_oid_17_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-17-spaces",
        sql,
        17,
        &[32, 9, 10, 32],
        &[32, 9, 10, 32],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 50, 48, 48, 57, 48, 97, 50, 48]],
            &[&[0, 1, 0, 0, 0, 4, 32, 9, 10, 32]],
        ],
    );
}

#[test]
fn should_decode_spaces_given_oid_25_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteainput-param-25-spaces",
        sql,
        25,
        &[32, 9, 10, 32],
        &[32, 9, 10, 32],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 50, 48, 48, 57, 48, 97, 50, 48]],
            &[&[0, 1, 0, 0, 0, 4, 32, 9, 10, 32]],
        ],
    );
}
