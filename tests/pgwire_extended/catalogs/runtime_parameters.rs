//! Explicit primitive parameters preserve the compression helper contract.
use crate::support_values_casts;

#[test]
fn should_report_compression_text_given_typed_parameters_when_input_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_column_compression($1) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "runtime-compression-text",
        sql,
        25,
        &[112, 108, 97, 105, 110],
        &[112, 108, 97, 105, 110],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_report_compression_int4_given_typed_parameters_when_input_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_column_compression($1) AS output";
    // Act
    let batches =
        support_values_casts::parameters("runtime-compression-int4", sql, 23, &[49], &[0, 0, 0, 1]);
    // Assert
    support_values_casts::assert_parameter(&batches, 23);
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_report_compression_int8_given_typed_parameters_when_input_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_column_compression($1) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "runtime-compression-int8",
        sql,
        20,
        &[49],
        &[0, 0, 0, 0, 0, 0, 0, 1],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 20);
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_report_compression_boolean_given_typed_parameters_when_input_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_column_compression($1) AS output";
    // Act
    let batches =
        support_values_casts::parameters("runtime-compression-boolean", sql, 16, &[116], &[1]);
    // Assert
    support_values_casts::assert_parameter(&batches, 16);
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_report_compression_bytea_given_typed_parameters_when_input_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_column_compression($1) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "runtime-compression-bytea",
        sql,
        17,
        &[92, 120, 48, 48, 102, 102],
        &[0, 255],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}
