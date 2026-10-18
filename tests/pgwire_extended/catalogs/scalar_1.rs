//! PostgreSQL 18.6 catalog scalar payload and descriptor witnesses.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_encoding_latin2_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('latin2') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_latin2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 57]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 9]],
        ],
    );
}

#[test]
fn should_preserve_encoding_latin3_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('latin3') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_latin3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 10]],
        ],
    );
}

#[test]
fn should_preserve_encoding_latin4_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('latin4') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_latin4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 11]],
        ],
    );
}

#[test]
fn should_preserve_encoding_latin5_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('latin5') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_latin5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 50]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 12]],
        ],
    );
}

#[test]
fn should_preserve_encoding_latin6_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('latin6') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_latin6", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 51]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 13]],
        ],
    );
}

#[test]
fn should_preserve_encoding_latin7_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('latin7') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_latin7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 52]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 14]],
        ],
    );
}

#[test]
fn should_preserve_encoding_latin8_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('latin8') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_latin8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 53]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 15]],
        ],
    );
}

#[test]
fn should_preserve_encoding_latin9_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('latin9') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_latin9", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 54]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 16]],
        ],
    );
}

#[test]
fn should_preserve_encoding_mskanji_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('mskanji') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_mskanji", sql);
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
fn should_preserve_encoding_muleinternal_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('muleinternal') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_muleinternal", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_encoding_shiftjis_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('shiftjis') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_shiftjis", sql);
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
fn should_preserve_encoding_shiftjis2004_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('shiftjis2004') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_shiftjis2004", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 52, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 41]],
        ],
    );
}

#[test]
fn should_preserve_encoding_sjis_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('sjis') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_sjis", sql);
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
fn should_preserve_encoding_sqlascii_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('sqlascii') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_sqlascii", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_encoding_tcvn_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('tcvn') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_tcvn", sql);
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
fn should_preserve_encoding_tcvn5712_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('tcvn5712') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_tcvn5712", sql);
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
fn should_preserve_encoding_uhc_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('uhc') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_uhc", sql);
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
fn should_preserve_encoding_unicode_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('unicode') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_unicode", sql);
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
fn should_preserve_encoding_utf8_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('utf8') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_utf8", sql);
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
fn should_preserve_encoding_vscii_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('vscii') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_vscii", sql);
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
fn should_preserve_encoding_win_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('win') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_win", sql);
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
fn should_preserve_encoding_win1250_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('win1250') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_win1250", sql);
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
fn should_preserve_encoding_win1251_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('win1251') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_win1251", sql);
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
fn should_preserve_encoding_win1252_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('win1252') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_win1252", sql);
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
fn should_preserve_encoding_win1253_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('win1253') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_win1253", sql);
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
fn should_preserve_encoding_win1254_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('win1254') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_win1254", sql);
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
fn should_preserve_encoding_win1255_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('win1255') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_win1255", sql);
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
fn should_preserve_encoding_win1256_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('win1256') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_win1256", sql);
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
fn should_preserve_encoding_win1257_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('win1257') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_win1257", sql);
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
fn should_preserve_encoding_win1258_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('win1258') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_win1258", sql);
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
