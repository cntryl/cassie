//! Native BYTEA zero extension and full-width signed decoding.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_decode_0_empty_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_0_empty", sql);
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
fn should_decode_1_zero_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x00'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_1_zero", sql);
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
fn should_decode_1_ones_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\xff'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_1_ones", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 50, 53, 53]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 255]],
        ],
    );
}

#[test]
fn should_decode_1_high_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x80'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_1_high", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 49, 50, 56]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 128]],
        ],
    );
}

#[test]
fn should_decode_2_zero_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x0000'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_2_zero", sql);
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
fn should_decode_2_ones_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\xffff'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_2_ones", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 54, 53, 53, 51, 53]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 255, 255]],
        ],
    );
}

#[test]
fn should_decode_2_high_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x8000'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_2_high", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 51, 50, 55, 54, 56]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 128, 0]],
        ],
    );
}

#[test]
fn should_decode_3_zero_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x000000'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_3_zero", sql);
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
fn should_decode_3_ones_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\xffffff'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_3_ones", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 49, 54, 55, 55, 55, 50, 49, 53]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_decode_3_high_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x800000'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_3_high", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 7, 56, 51, 56, 56, 54, 48, 56]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 128, 0, 0]],
        ],
    );
}

#[test]
fn should_decode_4_zero_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x00000000'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_4_zero", sql);
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
fn should_decode_4_ones_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\xffffffff'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_4_ones", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 52, 50, 57, 52, 57, 54, 55, 50, 57, 53]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_decode_4_high_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x80000000'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_4_high", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 50, 49, 52, 55, 52, 56, 51, 54, 52, 56]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 128, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_decode_5_zero_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x0000000000'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_5_zero", sql);
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
fn should_decode_5_ones_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\xffffffffff'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_5_ones", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 13, 49, 48, 57, 57, 53, 49, 49, 54, 50, 55, 55, 55, 53,
            ]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 255, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_decode_5_high_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x8000000000'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_5_high", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 12, 53, 52, 57, 55, 53, 53, 56, 49, 51, 56, 56, 56,
            ]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 128, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_decode_6_zero_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x000000000000'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_6_zero", sql);
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
fn should_decode_6_ones_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\xffffffffffff'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_6_ones", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 15, 50, 56, 49, 52, 55, 52, 57, 55, 54, 55, 49, 48, 54, 53, 53,
            ]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 255, 255, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_decode_6_high_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x800000000000'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_6_high", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 15, 49, 52, 48, 55, 51, 55, 52, 56, 56, 51, 53, 53, 51, 50, 56,
            ]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 128, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_decode_7_zero_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x00000000000000'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_7_zero", sql);
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
fn should_decode_7_ones_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\xffffffffffffff'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_7_ones", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 17, 55, 50, 48, 53, 55, 53, 57, 52, 48, 51, 55, 57, 50, 55, 57, 51,
                53,
            ]],
            &[&[0, 1, 0, 0, 0, 8, 0, 255, 255, 255, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_decode_7_high_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x80000000000000'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_7_high", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 17, 51, 54, 48, 50, 56, 55, 57, 55, 48, 49, 56, 57, 54, 51, 57, 54,
                56,
            ]],
            &[&[0, 1, 0, 0, 0, 8, 0, 128, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_decode_8_zero_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x0000000000000000'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_8_zero", sql);
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
fn should_decode_8_ones_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\xffffffffffffffff'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_8_ones", sql);
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
fn should_decode_8_high_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x8000000000000000'::BYTEA::INT8 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int8_8_high", sql);
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
