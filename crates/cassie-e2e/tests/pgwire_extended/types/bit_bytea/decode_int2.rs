//! Native BYTEA zero extension and full-width signed decoding.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_decode_0_empty_given_int2_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x'::BYTEA::INT2 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int2_0_empty", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [&[&[0, 1, 0, 0, 0, 1, 48]], &[&[0, 1, 0, 0, 0, 2, 0, 0]]],
    );
}

#[test]
fn should_decode_1_zero_given_int2_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x00'::BYTEA::INT2 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int2_1_zero", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [&[&[0, 1, 0, 0, 0, 1, 48]], &[&[0, 1, 0, 0, 0, 2, 0, 0]]],
    );
}

#[test]
fn should_decode_1_ones_given_int2_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\xff'::BYTEA::INT2 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int2_1_ones", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 50, 53, 53]],
            &[&[0, 1, 0, 0, 0, 2, 0, 255]],
        ],
    );
}

#[test]
fn should_decode_1_high_given_int2_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x80'::BYTEA::INT2 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int2_1_high", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 49, 50, 56]],
            &[&[0, 1, 0, 0, 0, 2, 0, 128]],
        ],
    );
}

#[test]
fn should_decode_2_zero_given_int2_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x0000'::BYTEA::INT2 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int2_2_zero", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [&[&[0, 1, 0, 0, 0, 1, 48]], &[&[0, 1, 0, 0, 0, 2, 0, 0]]],
    );
}

#[test]
fn should_decode_2_ones_given_int2_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\xffff'::BYTEA::INT2 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int2_2_ones", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 49]],
            &[&[0, 1, 0, 0, 0, 2, 255, 255]],
        ],
    );
}

#[test]
fn should_decode_2_high_given_int2_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let sql = "SELECT '\\x8000'::BYTEA::INT2 AS output";
    // Act
    let batches = planning::pruned("byteacasts-decode-int2_2_high", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 45, 51, 50, 55, 54, 56]],
            &[&[0, 1, 0, 0, 0, 2, 128, 0]],
        ],
    );
}
