//! PostgreSQL 18.6 catalog scalar payload and descriptor witnesses.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_encoding_abc_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('abc') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_abc", sql);
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
fn should_preserve_encoding_alt_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('alt') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_alt", sql);
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
fn should_preserve_encoding_big5_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('big5') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_big5", sql);
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
fn should_preserve_encoding_euccn_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('euccn') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_euccn", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 50]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 2]],
        ],
    );
}

#[test]
fn should_preserve_encoding_eucjis2004_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('eucjis2004') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_eucjis2004", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 53]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 5]],
        ],
    );
}

#[test]
fn should_preserve_encoding_eucjp_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('eucjp') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_eucjp", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_preserve_encoding_euckr_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('euckr') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_euckr", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 51]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 3]],
        ],
    );
}

#[test]
fn should_preserve_encoding_euctw_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('euctw') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_euctw", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 52]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 4]],
        ],
    );
}

#[test]
fn should_preserve_encoding_gb18030_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('gb18030') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_gb18030", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 51, 57]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 39]],
        ],
    );
}

#[test]
fn should_preserve_encoding_gbk_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('gbk') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_gbk", sql);
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
fn should_preserve_encoding_iso88591_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('iso88591') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_iso88591", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_preserve_encoding_iso885910_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('iso885910') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_iso885910", sql);
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
fn should_preserve_encoding_iso885913_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('iso885913') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_iso885913", sql);
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
fn should_preserve_encoding_iso885914_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('iso885914') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_iso885914", sql);
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
fn should_preserve_encoding_iso885915_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('iso885915') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_iso885915", sql);
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
fn should_preserve_encoding_iso885916_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('iso885916') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_iso885916", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 17]],
        ],
    );
}

#[test]
fn should_preserve_encoding_iso88592_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('iso88592') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_iso88592", sql);
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
fn should_preserve_encoding_iso88593_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('iso88593') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_iso88593", sql);
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
fn should_preserve_encoding_iso88594_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('iso88594') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_iso88594", sql);
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
fn should_preserve_encoding_iso88595_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('iso88595') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_iso88595", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 50, 53]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 25]],
        ],
    );
}

#[test]
fn should_preserve_encoding_iso88596_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('iso88596') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_iso88596", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 50, 54]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 26]],
        ],
    );
}

#[test]
fn should_preserve_encoding_iso88597_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('iso88597') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_iso88597", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 50, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 27]],
        ],
    );
}

#[test]
fn should_preserve_encoding_iso88598_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('iso88598') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_iso88598", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 50, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 28]],
        ],
    );
}

#[test]
fn should_preserve_encoding_iso88599_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('iso88599') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_iso88599", sql);
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
fn should_preserve_encoding_johab_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('johab') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_johab", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 52, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 40]],
        ],
    );
}

#[test]
fn should_preserve_encoding_koi8_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('koi8') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_koi8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 50, 50]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 22]],
        ],
    );
}

#[test]
fn should_preserve_encoding_koi8r_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('koi8r') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_koi8r", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 50, 50]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 22]],
        ],
    );
}

#[test]
fn should_preserve_encoding_koi8u_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('koi8u') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_koi8u", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 51, 52]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 34]],
        ],
    );
}

#[test]
fn should_preserve_encoding_latin1_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('latin1') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_latin1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_preserve_encoding_latin10_given_native_scalar_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_char_to_encoding('latin10') AS output";
    // Act
    let batches = planning::pruned("catalog-scalar-encoding_latin10", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 17]],
        ],
    );
}
