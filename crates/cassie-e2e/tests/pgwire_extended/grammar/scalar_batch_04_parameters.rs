//! Native parameter identities and payloads across all four format combinations.
use crate::support_values_casts;

#[test]
fn should_evaluate_abs_small_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT @ $1 AS output";
    // Act
    let batches = support_values_casts::parameters(
        "unary-math-param-abs_small",
        sql,
        21,
        &[45, 51, 50, 55, 54, 55],
        &[128, 1],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 21);
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
fn should_evaluate_abs_int_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT @ $1 AS output";
    // Act
    let batches = support_values_casts::parameters(
        "unary-math-param-abs_int",
        sql,
        23,
        &[45, 50, 49, 52, 55, 52, 56, 51, 54, 52, 55],
        &[128, 0, 0, 1],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 23);
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
fn should_evaluate_abs_big_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT @ $1 AS output";
    // Act
    let batches = support_values_casts::parameters(
        "unary-math-param-abs_big",
        sql,
        20,
        &[
            45, 57, 50, 50, 51, 51, 55, 50, 48, 51, 54, 56, 53, 52, 55, 55, 53, 56, 48, 55,
        ],
        &[128, 0, 0, 0, 0, 0, 0, 1],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 20);
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

#[test]
fn should_evaluate_abs_float_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT @ $1 AS output";
    // Act
    let batches = support_values_casts::parameters(
        "unary-math-param-abs_float",
        sql,
        701,
        &[45, 49, 46, 53],
        &[191, 248, 0, 0, 0, 0, 0, 0],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 701);
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 49, 46, 53]],
            &[&[0, 1, 0, 0, 0, 8, 63, 248, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_evaluate_root_int_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT |/ $1 AS output";
    // Act
    let batches = support_values_casts::parameters(
        "unary-math-param-root_int",
        sql,
        23,
        &[50, 53],
        &[0, 0, 0, 25],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 23);
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 53]],
            &[&[0, 1, 0, 0, 0, 8, 64, 20, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_evaluate_root_inferred_given_explicit_parameters_when_input_and_output_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT |/ $1 AS output";
    // Act
    let batches = support_values_casts::parameters(
        "unary-math-param-root_inferred",
        sql,
        0,
        &[50, 53],
        &[64, 57, 0, 0, 0, 0, 0, 0],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 701);
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 53]],
            &[&[0, 1, 0, 0, 0, 8, 64, 20, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_evaluate_cube_big_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT ||/ $1 AS output";
    // Act
    let batches = support_values_casts::parameters(
        "unary-math-param-cube_big",
        sql,
        20,
        &[45, 56],
        &[255, 255, 255, 255, 255, 255, 255, 248],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 20);
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 50]],
            &[&[0, 1, 0, 0, 0, 8, 192, 0, 0, 0, 0, 0, 0, 0]],
        ],
    );
}
