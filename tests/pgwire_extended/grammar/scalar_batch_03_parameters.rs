//! Native parameter identities and payloads across all four format combinations.
use crate::support_values_casts;

#[test]
fn should_shift_left_small_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1 << 16 AS output";
    // Act
    let batches =
        support_values_casts::parameters("bitwise-param-left_small", sql, 21, &[49], &[0, 1]);
    // Assert
    support_values_casts::assert_parameter(&batches, 21);
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [&[&[0, 1, 0, 0, 0, 1, 48]], &[&[0, 1, 0, 0, 0, 2, 0, 0]]],
    );
}

#[test]
fn should_shift_left_int_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1 << (-1) AS output";
    // Act
    let batches =
        support_values_casts::parameters("bitwise-param-left_int", sql, 23, &[49], &[0, 0, 0, 1]);
    // Assert
    support_values_casts::assert_parameter(&batches, 23);
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

#[test]
fn should_shift_left_big_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1 << 63 AS output";
    // Act
    let batches = support_values_casts::parameters(
        "bitwise-param-left_big",
        sql,
        20,
        &[49],
        &[0, 0, 0, 0, 0, 0, 0, 1],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 20);
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
fn should_shift_count_int_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT 1::SMALLINT << $1 AS output";
    // Act
    let batches = support_values_casts::parameters(
        "bitwise-param-count_int",
        sql,
        23,
        &[54, 53, 53, 51, 54],
        &[0, 1, 0, 0],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 23);
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [&[&[0, 1, 0, 0, 0, 1, 49]], &[&[0, 1, 0, 0, 0, 2, 0, 1]]],
    );
}

#[test]
fn should_shift_count_small_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT 1::SMALLINT << $1 AS output";
    // Act
    let batches =
        support_values_casts::parameters("bitwise-param-count_small", sql, 21, &[49, 54], &[0, 16]);
    // Assert
    support_values_casts::assert_parameter(&batches, 21);
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [&[&[0, 1, 0, 0, 0, 1, 48]], &[&[0, 1, 0, 0, 0, 2, 0, 0]]],
    );
}

#[test]
fn should_shift_inferred_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1 << 1 AS output";
    // Act
    let batches =
        support_values_casts::parameters("bitwise-param-inferred", sql, 0, &[55], &[0, 0, 0, 7]);
    // Assert
    support_values_casts::assert_parameter(&batches, 23);
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 52]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 14]],
        ],
    );
}
