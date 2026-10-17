//! Native parameter identities and payloads across all four format combinations.
use crate::support_values_casts;

#[test]
fn should_evaluate_int_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1 BETWEEN SYMMETRIC 3 AND 1 AS output";
    // Act
    let batches =
        support_values_casts::parameters("between-param-int", sql, 23, &[50], &[0, 0, 0, 2]);
    // Assert
    support_values_casts::assert_parameter(&batches, 23);
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_evaluate_inferred_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1 BETWEEN SYMMETRIC 3 AND 1 AS output";
    // Act
    let batches =
        support_values_casts::parameters("between-param-inferred", sql, 0, &[50], &[0, 0, 0, 2]);
    // Assert
    support_values_casts::assert_parameter(&batches, 23);
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_evaluate_negative_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1 BETWEEN SYMMETRIC 3 AND 1 AS output";
    // Act
    let batches = support_values_casts::parameters(
        "between-param-negative",
        sql,
        23,
        &[45, 50],
        &[255, 255, 255, 254],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 23);
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_evaluate_bool_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1 BETWEEN SYMMETRIC TRUE AND FALSE AS output";
    // Act
    let batches = support_values_casts::parameters(
        "between-param-bool",
        sql,
        16,
        &[116, 114, 117, 101],
        &[1],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 16);
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_evaluate_float_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1 BETWEEN SYMMETRIC 3 AND 1 AS output";
    // Act
    let batches = support_values_casts::parameters(
        "between-param-float",
        sql,
        701,
        &[50],
        &[64, 0, 0, 0, 0, 0, 0, 0],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 701);
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}
