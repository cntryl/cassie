//! Native integer and BYTEA parameter casts across wire formats.
use crate::support_values_casts;

#[test]
fn should_cast_int2_0_encode_given_retained_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteacasts-param-int2_0_encode",
        sql,
        21,
        &[45, 49],
        &[255, 255],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 21);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 92, 120, 102, 102, 102, 102]],
            &[&[0, 1, 0, 0, 0, 2, 255, 255]],
        ],
    );
}

#[test]
fn should_cast_int2_0_decode_given_retained_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::INT2 AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteacasts-param-int2_0_decode",
        sql,
        17,
        &[92, 120, 102, 102, 102, 102],
        &[255, 255],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
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
fn should_cast_int2_1_encode_given_retained_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches =
        support_values_casts::parameters("byteacasts-param-int2_1_encode", sql, 21, &[48], &[0, 0]);
    // Assert
    support_values_casts::assert_parameter(&batches, 21);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 92, 120, 48, 48, 48, 48]],
            &[&[0, 1, 0, 0, 0, 2, 0, 0]],
        ],
    );
}

#[test]
fn should_cast_int2_1_decode_given_retained_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::INT2 AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteacasts-param-int2_1_decode",
        sql,
        17,
        &[92, 120, 48, 48, 48, 48],
        &[0, 0],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [&[&[0, 1, 0, 0, 0, 1, 48]], &[&[0, 1, 0, 0, 0, 2, 0, 0]]],
    );
}

#[test]
fn should_cast_int2_2_encode_given_retained_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteacasts-param-int2_2_encode",
        sql,
        21,
        &[51, 50, 55, 54, 55],
        &[127, 255],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 21);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 92, 120, 55, 102, 102, 102]],
            &[&[0, 1, 0, 0, 0, 2, 127, 255]],
        ],
    );
}

#[test]
fn should_cast_int2_2_decode_given_retained_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::INT2 AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteacasts-param-int2_2_decode",
        sql,
        17,
        &[92, 120, 55, 102, 102, 102],
        &[127, 255],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 51, 50, 55, 54, 55]],
            &[&[0, 1, 0, 0, 0, 2, 127, 255]],
        ],
    );
}

#[test]
fn should_cast_int4_0_encode_given_retained_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteacasts-param-int4_0_encode",
        sql,
        23,
        &[45, 49],
        &[255, 255, 255, 255],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 23);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 102, 102, 102, 102, 102, 102, 102, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_cast_int4_0_decode_given_retained_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::INT4 AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteacasts-param-int4_0_decode",
        sql,
        17,
        &[92, 120, 102, 102, 102, 102, 102, 102, 102, 102],
        &[255, 255, 255, 255],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
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
fn should_cast_int4_1_encode_given_retained_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteacasts-param-int4_1_encode",
        sql,
        23,
        &[48],
        &[0, 0, 0, 0],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 23);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 48, 48, 48, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_cast_int4_1_decode_given_retained_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::INT4 AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteacasts-param-int4_1_decode",
        sql,
        17,
        &[92, 120, 48, 48, 48, 48, 48, 48, 48, 48],
        &[0, 0, 0, 0],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
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
fn should_cast_int4_2_encode_given_retained_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteacasts-param-int4_2_encode",
        sql,
        23,
        &[50, 49, 52, 55, 52, 56, 51, 54, 52, 55],
        &[127, 255, 255, 255],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 23);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 55, 102, 102, 102, 102, 102, 102, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 127, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_cast_int4_2_decode_given_retained_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::INT4 AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteacasts-param-int4_2_decode",
        sql,
        17,
        &[92, 120, 55, 102, 102, 102, 102, 102, 102, 102],
        &[127, 255, 255, 255],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 50, 49, 52, 55, 52, 56, 51, 54, 52, 55]],
            &[&[0, 1, 0, 0, 0, 4, 127, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_cast_int8_0_encode_given_retained_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteacasts-param-int8_0_encode",
        sql,
        20,
        &[45, 49],
        &[255, 255, 255, 255, 255, 255, 255, 255],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 20);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 18, 92, 120, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102,
                102, 102, 102, 102, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 8, 255, 255, 255, 255, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_cast_int8_0_decode_given_retained_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::INT8 AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteacasts-param-int8_0_decode",
        sql,
        17,
        &[
            92, 120, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102,
        ],
        &[255, 255, 255, 255, 255, 255, 255, 255],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
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
fn should_cast_int8_1_encode_given_retained_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteacasts-param-int8_1_encode",
        sql,
        20,
        &[48],
        &[0, 0, 0, 0, 0, 0, 0, 0],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 20);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 18, 92, 120, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48,
                48, 48,
            ]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_cast_int8_1_decode_given_retained_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::INT8 AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteacasts-param-int8_1_decode",
        sql,
        17,
        &[
            92, 120, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48,
        ],
        &[0, 0, 0, 0, 0, 0, 0, 0],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
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
fn should_cast_int8_2_encode_given_retained_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::BYTEA AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteacasts-param-int8_2_encode",
        sql,
        20,
        &[
            57, 50, 50, 51, 51, 55, 50, 48, 51, 54, 56, 53, 52, 55, 55, 53, 56, 48, 55,
        ],
        &[127, 255, 255, 255, 255, 255, 255, 255],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 20);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 18, 92, 120, 55, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102,
                102, 102, 102, 102, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 8, 127, 255, 255, 255, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_cast_int8_2_decode_given_retained_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::INT8 AS output";
    // Act
    let batches = support_values_casts::parameters(
        "byteacasts-param-int8_2_decode",
        sql,
        17,
        &[
            92, 120, 55, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102,
        ],
        &[127, 255, 255, 255, 255, 255, 255, 255],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
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
