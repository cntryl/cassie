//! PostgreSQL 18.6 catalog scalar payload and descriptor witnesses.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_encoding_win866_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('win866') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_win866", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 50, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 20]],
        ],
    );
}

#[test]
fn should_preserve_encoding_win874_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('win874') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_win874", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 50, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 21]],
        ],
    );
}

#[test]
fn should_preserve_encoding_win932_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('win932') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_win932", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 51, 53]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 35]],
        ],
    );
}

#[test]
fn should_preserve_encoding_win936_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('win936') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_win936", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 51, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 37]],
        ],
    );
}

#[test]
fn should_preserve_encoding_win949_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('win949') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_win949", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 51, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 38]],
        ],
    );
}

#[test]
fn should_preserve_encoding_win950_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('win950') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_win950", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 51, 54]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 36]],
        ],
    );
}

#[test]
fn should_preserve_encoding_windows1250_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('windows1250') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_windows1250", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 50, 57]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 29]],
        ],
    );
}

#[test]
fn should_preserve_encoding_windows1251_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('windows1251') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_windows1251", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 50, 51]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 23]],
        ],
    );
}

#[test]
fn should_preserve_encoding_windows1252_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('windows1252') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_windows1252", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 50, 52]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 24]],
        ],
    );
}

#[test]
fn should_preserve_encoding_windows1253_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('windows1253') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_windows1253", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 51, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 30]],
        ],
    );
}

#[test]
fn should_preserve_encoding_windows1254_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('windows1254') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_windows1254", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 51, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 31]],
        ],
    );
}

#[test]
fn should_preserve_encoding_windows1255_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('windows1255') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_windows1255", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 51, 50]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 32]],
        ],
    );
}

#[test]
fn should_preserve_encoding_windows1256_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('windows1256') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_windows1256", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 18]],
        ],
    );
}

#[test]
fn should_preserve_encoding_windows1257_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('windows1257') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_windows1257", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 51, 51]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 33]],
        ],
    );
}

#[test]
fn should_preserve_encoding_windows1258_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('windows1258') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_windows1258", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 57]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 19]],
        ],
    );
}

#[test]
fn should_preserve_encoding_windows866_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('windows866') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_windows866", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 50, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 20]],
        ],
    );
}

#[test]
fn should_preserve_encoding_windows874_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('windows874') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_windows874", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 50, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 21]],
        ],
    );
}

#[test]
fn should_preserve_encoding_windows932_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('windows932') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_windows932", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 51, 53]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 35]],
        ],
    );
}

#[test]
fn should_preserve_encoding_windows936_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('windows936') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_windows936", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 51, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 37]],
        ],
    );
}

#[test]
fn should_preserve_encoding_windows949_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('windows949') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_windows949", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 51, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 38]],
        ],
    );
}

#[test]
fn should_preserve_encoding_windows950_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('windows950') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_windows950", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 51, 54]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 36]],
        ],
    );
}

#[test]
fn should_preserve_encoding_control_0_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_char_to_encoding('U-T_F 8') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_control_0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 54]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 6]],
        ],
    );
}

#[test]
fn should_preserve_encoding_control_1_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_char_to_encoding('windows-1252') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_control_1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 50, 52]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 24]],
        ],
    );
}

#[test]
fn should_preserve_encoding_control_2_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_char_to_encoding('UTF8                                                           ') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_control_2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 54]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 6]],
        ],
    );
}

#[test]
fn should_preserve_encoding_control_3_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_char_to_encoding('UTF8                                                            ') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_control_3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 54]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 6]],
        ],
    );
}

#[test]
fn should_preserve_encoding_control_4_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_char_to_encoding('') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_control_4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 49]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_encoding_control_5_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_char_to_encoding('not-an-encoding') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_control_5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 49]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_encoding_control_6_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_char_to_encoding('UTFé8') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_control_6", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 49]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_encoding_control_7_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_char_to_encoding('💚UTF8') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_control_7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 49]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_encoding_control_8_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_char_to_encoding(NULL::text) AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_control_8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}
