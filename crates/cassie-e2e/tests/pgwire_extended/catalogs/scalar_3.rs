//! PostgreSQL 18.6 catalog scalar payload and descriptor witnesses.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_bytes_0_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('0') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 48]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_bytes_1_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('1') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_preserve_bytes_2_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('-1') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 49]],
            &[&[0, 1, 0, 0, 0, 8, 255, 255, 255, 255, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_bytes_3_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('0.5') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_preserve_bytes_4_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('-0.5') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 49]],
            &[&[0, 1, 0, 0, 0, 8, 255, 255, 255, 255, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_bytes_5_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('1.499999999999999999') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_preserve_bytes_6_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('1.5') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_6", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 50]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 2]],
        ],
    );
}

#[test]
fn should_preserve_bytes_7_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('-1.5') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 50]],
            &[&[0, 1, 0, 0, 0, 8, 255, 255, 255, 255, 255, 255, 255, 254]],
        ],
    );
}

#[test]
fn should_preserve_bytes_8_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('1.5 MB') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 7, 49, 53, 55, 50, 56, 54, 52]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 24, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_bytes_9_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('1 kB') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_9", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 49, 48, 50, 52]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 4, 0]],
        ],
    );
}

#[test]
fn should_preserve_bytes_10_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('1KB') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_10", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 49, 48, 50, 52]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 4, 0]],
        ],
    );
}

#[test]
fn should_preserve_bytes_11_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('1 b') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_11", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_preserve_bytes_12_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('1bytes') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_12", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_preserve_bytes_13_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('1pB') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_13", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 16, 49, 49, 50, 53, 56, 57, 57, 57, 48, 54, 56, 52, 50, 54, 50, 52,
            ]],
            &[&[0, 1, 0, 0, 0, 8, 0, 4, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_bytes_14_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('1 TB') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_14", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 13, 49, 48, 57, 57, 53, 49, 49, 54, 50, 55, 55, 55, 54,
            ]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 1, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_bytes_15_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('1 GB') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_15", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 49, 48, 55, 51, 55, 52, 49, 56, 50, 52]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 64, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_bytes_16_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('  +.5 kB  ') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_16", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 53, 49, 50]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 2, 0]],
        ],
    );
}

#[test]
fn should_preserve_bytes_17_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('1. kB') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_17", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 49, 48, 50, 52]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 4, 0]],
        ],
    );
}

#[test]
fn should_preserve_bytes_18_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('1e3') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_18", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 49, 48, 48, 48]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 3, 232]],
        ],
    );
}

#[test]
fn should_preserve_bytes_19_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('1e-3 MB') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_19", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 49, 48, 52, 57]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 4, 25]],
        ],
    );
}

#[test]
fn should_preserve_bytes_20_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('1e+3 B') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_20", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 49, 48, 48, 48]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 3, 232]],
        ],
    );
}

#[test]
fn should_preserve_bytes_21_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('-0.00048828125 kB') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_21", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 49]],
            &[&[0, 1, 0, 0, 0, 8, 255, 255, 255, 255, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_bytes_22_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('0.000488281249999999999999 kB') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_22", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 48]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_bytes_23_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('0.00048828125 kB') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_23", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_preserve_bytes_24_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('9223372036854775807') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_24", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 19, 57, 50, 50, 51, 51, 55, 50, 48, 51, 54, 56, 53, 52, 55, 55, 53,
                56, 48, 55,
            ]],
            &[&[0, 1, 0, 0, 0, 8, 127, 255, 255, 255, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_bytes_25_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('-9223372036854775808') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_25", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 20, 45, 57, 50, 50, 51, 51, 55, 50, 48, 51, 54, 56, 53, 52, 55, 55,
                53, 56, 48, 56,
            ]],
            &[&[0, 1, 0, 0, 0, 8, 128, 0, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_bytes_26_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('9223372036854775806.5') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_26", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 19, 57, 50, 50, 51, 51, 55, 50, 48, 51, 54, 56, 53, 52, 55, 55, 53,
                56, 48, 55,
            ]],
            &[&[0, 1, 0, 0, 0, 8, 127, 255, 255, 255, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_bytes_27_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('-9223372036854775807.5') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_27", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 20, 45, 57, 50, 50, 51, 51, 55, 50, 48, 51, 54, 56, 53, 52, 55, 55,
                53, 56, 48, 56,
            ]],
            &[&[0, 1, 0, 0, 0, 8, 128, 0, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_bytes_28_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('0.00000000000000088817841970012523233890533447265625 PB') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_28", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_preserve_bytes_29_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('00000000000000000000000000000000000000000000000000000000001 B') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_29", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1]],
        ],
    );
}
