//! Native BYTEA input conversion, descriptors and both output formats.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_decode_empty_given_cte_input_when_bytea_is_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES (''::BYTEA)) SELECT v AS output FROM q";
    // Act
    let batches = planning::pruned("byteainput-cte-empty", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [&[&[0, 1, 0, 0, 0, 2, 92, 120]], &[&[0, 1, 0, 0, 0, 0]]],
    );
}

#[test]
fn should_decode_ascii_given_cte_input_when_bytea_is_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('abc'::BYTEA)) SELECT v AS output FROM q";
    // Act
    let batches = planning::pruned("byteainput-cte-ascii", sql);
    // Assert
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
fn should_decode_utf8_given_cte_input_when_bytea_is_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('é🙂'::BYTEA)) SELECT v AS output FROM q";
    // Act
    let batches = planning::pruned("byteainput-cte-utf8", sql);
    // Assert
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
fn should_decode_nul_given_cte_input_when_bytea_is_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\000'::BYTEA)) SELECT v AS output FROM q";
    // Act
    let batches = planning::pruned("byteainput-cte-nul", sql);
    // Assert
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
fn should_decode_high_given_cte_input_when_bytea_is_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\377'::BYTEA)) SELECT v AS output FROM q";
    // Act
    let batches = planning::pruned("byteainput-cte-high", sql);
    // Assert
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
fn should_decode_slash_given_cte_input_when_bytea_is_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\\\'::BYTEA)) SELECT v AS output FROM q";
    // Act
    let batches = planning::pruned("byteainput-cte-slash", sql);
    // Assert
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
fn should_decode_mixed_given_cte_input_when_bytea_is_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('a\\000b\\\\c\\377'::BYTEA)) SELECT v AS output FROM q";
    // Act
    let batches = planning::pruned("byteainput-cte-mixed", sql);
    // Assert
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
fn should_decode_octal_ascii_given_cte_input_when_bytea_is_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\101\\102\\103'::BYTEA)) SELECT v AS output FROM q";
    // Act
    let batches = planning::pruned("byteainput-cte-octal_ascii", sql);
    // Assert
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
fn should_decode_octal_low_given_cte_input_when_bytea_is_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\001\\007\\010\\011\\012\\015\\037\\177'::BYTEA)) SELECT v AS output FROM q";
    // Act
    let batches = planning::pruned("byteainput-cte-octal_low", sql);
    // Assert
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
fn should_decode_hex_empty_given_cte_input_when_bytea_is_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\x'::BYTEA)) SELECT v AS output FROM q";
    // Act
    let batches = planning::pruned("byteainput-cte-hex_empty", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [&[&[0, 1, 0, 0, 0, 2, 92, 120]], &[&[0, 1, 0, 0, 0, 0]]],
    );
}

#[test]
fn should_decode_hex_case_given_cte_input_when_bytea_is_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\x00ABff'::BYTEA)) SELECT v AS output FROM q";
    // Act
    let batches = planning::pruned("byteainput-cte-hex_case", sql);
    // Assert
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
fn should_decode_hex_spaces_given_cte_input_when_bytea_is_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\x 00 AB ff '::BYTEA)) SELECT v AS output FROM q";
    // Act
    let batches = planning::pruned("byteainput-cte-hex_spaces", sql);
    // Assert
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
fn should_decode_hex_lines_given_cte_input_when_bytea_is_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\x00\n11\r22\t33'::BYTEA)) SELECT v AS output FROM q";
    // Act
    let batches = planning::pruned("byteainput-cte-hex_lines", sql);
    // Assert
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
fn should_decode_slash_tail_given_cte_input_when_bytea_is_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('abc\\\\'::BYTEA)) SELECT v AS output FROM q";
    // Act
    let batches = planning::pruned("byteainput-cte-slash_tail", sql);
    // Assert
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
fn should_decode_quote_given_cte_input_when_bytea_is_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('a''b'::BYTEA)) SELECT v AS output FROM q";
    // Act
    let batches = planning::pruned("byteainput-cte-quote", sql);
    // Assert
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
fn should_decode_spaces_given_cte_input_when_bytea_is_described() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES (' \t\n '::BYTEA)) SELECT v AS output FROM q";
    // Act
    let batches = planning::pruned("byteainput-cte-spaces", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 50, 48, 48, 57, 48, 97, 50, 48]],
            &[&[0, 1, 0, 0, 0, 4, 32, 9, 10, 32]],
        ],
    );
}
