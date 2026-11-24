//! PostgreSQL 18.6 catalog scalar payload and descriptor witnesses.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_bytes_30_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('1e-1000 PB') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_30", sql);
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
fn should_preserve_bytes_31_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes('0e1000') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_31", sql);
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
fn should_preserve_bytes_32_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_bytes(NULL::text) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-bytes_32", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_pretty_0_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((0)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 7, 48, 32, 98, 121, 116, 101, 115]],
            &[&[0, 1, 0, 0, 0, 7, 48, 32, 98, 121, 116, 101, 115]],
        ],
    );
}

#[test]
fn should_preserve_pretty_1_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((1)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 7, 49, 32, 98, 121, 116, 101, 115]],
            &[&[0, 1, 0, 0, 0, 7, 49, 32, 98, 121, 116, 101, 115]],
        ],
    );
}

#[test]
fn should_preserve_pretty_2_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((-1)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 45, 49, 32, 98, 121, 116, 101, 115]],
            &[&[0, 1, 0, 0, 0, 8, 45, 49, 32, 98, 121, 116, 101, 115]],
        ],
    );
}

#[test]
fn should_preserve_pretty_3_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((1023)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 49, 48, 50, 51, 32, 98, 121, 116, 101, 115,
            ]],
            &[&[
                0, 1, 0, 0, 0, 10, 49, 48, 50, 51, 32, 98, 121, 116, 101, 115,
            ]],
        ],
    );
}

#[test]
fn should_preserve_pretty_4_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((1024)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 49, 48, 50, 52, 32, 98, 121, 116, 101, 115,
            ]],
            &[&[
                0, 1, 0, 0, 0, 10, 49, 48, 50, 52, 32, 98, 121, 116, 101, 115,
            ]],
        ],
    );
}

#[test]
fn should_preserve_pretty_5_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((10239)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 11, 49, 48, 50, 51, 57, 32, 98, 121, 116, 101, 115,
            ]],
            &[&[
                0, 1, 0, 0, 0, 11, 49, 48, 50, 51, 57, 32, 98, 121, 116, 101, 115,
            ]],
        ],
    );
}

#[test]
fn should_preserve_pretty_6_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((10240)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_6", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 49, 48, 32, 107, 66]],
            &[&[0, 1, 0, 0, 0, 5, 49, 48, 32, 107, 66]],
        ],
    );
}

#[test]
fn should_preserve_pretty_7_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((-10239)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 12, 45, 49, 48, 50, 51, 57, 32, 98, 121, 116, 101, 115,
            ]],
            &[&[
                0, 1, 0, 0, 0, 12, 45, 49, 48, 50, 51, 57, 32, 98, 121, 116, 101, 115,
            ]],
        ],
    );
}

#[test]
fn should_preserve_pretty_8_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((-10240)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 45, 49, 48, 32, 107, 66]],
            &[&[0, 1, 0, 0, 0, 6, 45, 49, 48, 32, 107, 66]],
        ],
    );
}

#[test]
fn should_preserve_pretty_9_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((10751)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_9", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 49, 48, 32, 107, 66]],
            &[&[0, 1, 0, 0, 0, 5, 49, 48, 32, 107, 66]],
        ],
    );
}

#[test]
fn should_preserve_pretty_10_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((10752)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_10", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 49, 49, 32, 107, 66]],
            &[&[0, 1, 0, 0, 0, 5, 49, 49, 32, 107, 66]],
        ],
    );
}

#[test]
fn should_preserve_pretty_11_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((-10751)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_11", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 45, 49, 48, 32, 107, 66]],
            &[&[0, 1, 0, 0, 0, 6, 45, 49, 48, 32, 107, 66]],
        ],
    );
}

#[test]
fn should_preserve_pretty_12_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((-10752)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_12", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 45, 49, 49, 32, 107, 66]],
            &[&[0, 1, 0, 0, 0, 6, 45, 49, 49, 32, 107, 66]],
        ],
    );
}

#[test]
fn should_preserve_pretty_13_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((10485247)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_13", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 49, 48, 50, 51, 57, 32, 107, 66]],
            &[&[0, 1, 0, 0, 0, 8, 49, 48, 50, 51, 57, 32, 107, 66]],
        ],
    );
}

#[test]
fn should_preserve_pretty_14_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((10485248)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_14", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 49, 48, 32, 77, 66]],
            &[&[0, 1, 0, 0, 0, 5, 49, 48, 32, 77, 66]],
        ],
    );
}

#[test]
fn should_preserve_pretty_15_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((10485760)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_15", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 49, 48, 32, 77, 66]],
            &[&[0, 1, 0, 0, 0, 5, 49, 48, 32, 77, 66]],
        ],
    );
}

#[test]
fn should_preserve_pretty_16_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((-10485247)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_16", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 9, 45, 49, 48, 50, 51, 57, 32, 107, 66]],
            &[&[0, 1, 0, 0, 0, 9, 45, 49, 48, 50, 51, 57, 32, 107, 66]],
        ],
    );
}

#[test]
fn should_preserve_pretty_17_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((-10485248)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_17", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 45, 49, 48, 32, 77, 66]],
            &[&[0, 1, 0, 0, 0, 6, 45, 49, 48, 32, 77, 66]],
        ],
    );
}

#[test]
fn should_preserve_pretty_18_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((10737418240)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_18", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 49, 48, 32, 71, 66]],
            &[&[0, 1, 0, 0, 0, 5, 49, 48, 32, 71, 66]],
        ],
    );
}

#[test]
fn should_preserve_pretty_19_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((10995116277760)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_19", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 49, 48, 32, 84, 66]],
            &[&[0, 1, 0, 0, 0, 5, 49, 48, 32, 84, 66]],
        ],
    );
}

#[test]
fn should_preserve_pretty_20_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((11258999068426240)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_20", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 49, 48, 32, 80, 66]],
            &[&[0, 1, 0, 0, 0, 5, 49, 48, 32, 80, 66]],
        ],
    );
}

#[test]
fn should_preserve_pretty_21_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((9223372036854775807)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_21", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 7, 56, 49, 57, 50, 32, 80, 66]],
            &[&[0, 1, 0, 0, 0, 7, 56, 49, 57, 50, 32, 80, 66]],
        ],
    );
}

#[test]
fn should_preserve_pretty_22_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_size_pretty((-9223372036854775808)::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_22", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 45, 56, 49, 57, 50, 32, 80, 66]],
            &[&[0, 1, 0, 0, 0, 8, 45, 56, 49, 57, 50, 32, 80, 66]],
        ],
    );
}

#[test]
fn should_preserve_pretty_null_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_size_pretty(NULL::bigint) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-pretty_null", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}
