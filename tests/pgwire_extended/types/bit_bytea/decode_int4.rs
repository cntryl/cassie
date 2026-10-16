//! Native BYTEA zero extension and full-width signed decoding.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_decode_0_empty_given_int4_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x'::BYTEA::INT4 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int4_0_empty", sql);
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
fn should_decode_1_zero_given_int4_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x00'::BYTEA::INT4 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int4_1_zero", sql);
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
fn should_decode_1_ones_given_int4_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\xff'::BYTEA::INT4 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int4_1_ones", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 50, 53, 53]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 255]],
        ],
    );
}

#[test]
fn should_decode_1_high_given_int4_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x80'::BYTEA::INT4 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int4_1_high", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 49, 50, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 128]],
        ],
    );
}

#[test]
fn should_decode_2_zero_given_int4_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x0000'::BYTEA::INT4 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int4_2_zero", sql);
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
fn should_decode_2_ones_given_int4_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\xffff'::BYTEA::INT4 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int4_2_ones", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 54, 53, 53, 51, 53]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 255, 255]],
        ],
    );
}

#[test]
fn should_decode_2_high_given_int4_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x8000'::BYTEA::INT4 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int4_2_high", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 51, 50, 55, 54, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 128, 0]],
        ],
    );
}

#[test]
fn should_decode_3_zero_given_int4_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x000000'::BYTEA::INT4 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int4_3_zero", sql);
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
fn should_decode_3_ones_given_int4_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\xffffff'::BYTEA::INT4 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int4_3_ones", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 49, 54, 55, 55, 55, 50, 49, 53]],
            &[&[0, 1, 0, 0, 0, 4, 0, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_decode_3_high_given_int4_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x800000'::BYTEA::INT4 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int4_3_high", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 7, 56, 51, 56, 56, 54, 48, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 128, 0, 0]],
        ],
    );
}

#[test]
fn should_decode_4_zero_given_int4_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x00000000'::BYTEA::INT4 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int4_4_zero", sql);
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
fn should_decode_4_ones_given_int4_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\xffffffff'::BYTEA::INT4 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int4_4_ones", sql);
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
fn should_decode_4_high_given_int4_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x80000000'::BYTEA::INT4 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int4_4_high", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 11, 45, 50, 49, 52, 55, 52, 56, 51, 54, 52, 56,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 128, 0, 0, 0]],
        ],
    );
}
