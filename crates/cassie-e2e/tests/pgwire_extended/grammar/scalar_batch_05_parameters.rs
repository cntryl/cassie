//! Native parameter identities and payloads across all four format combinations.
use crate::support_values_casts;

#[test]
fn should_evaluate_true_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1 IS TRUE AS output";
    // Act
    let batches =
        support_values_casts::parameters("truth-param-true", sql, 16, &[116, 114, 117, 101], &[1]);
    // Assert
    support_values_casts::assert_parameter(&batches, 16);
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_evaluate_false_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1 IS FALSE AS output";
    // Act
    let batches = support_values_casts::parameters(
        "truth-param-false",
        sql,
        16,
        &[102, 97, 108, 115, 101],
        &[0],
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
fn should_evaluate_unknown_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1 IS UNKNOWN AS output";
    // Act
    let batches = support_values_casts::parameters(
        "truth-param-unknown",
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
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_evaluate_not_unknown_given_explicit_parameters_when_input_and_output_formats_are_crossed()
{
    // Arrange
    let sql = "SELECT $1 IS NOT UNKNOWN AS output";
    // Act
    let batches = support_values_casts::parameters(
        "truth-param-not_unknown",
        sql,
        16,
        &[102, 97, 108, 115, 101],
        &[0],
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
fn should_evaluate_inferred_true_given_explicit_parameters_when_input_and_output_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT $1 IS TRUE AS output";
    // Act
    let batches = support_values_casts::parameters(
        "truth-param-inferred_true",
        sql,
        0,
        &[121, 101, 115],
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
fn should_evaluate_inferred_false_given_explicit_parameters_when_input_and_output_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT $1 IS FALSE AS output";
    // Act
    let batches = support_values_casts::parameters(
        "truth-param-inferred_false",
        sql,
        0,
        &[111, 102, 102],
        &[0],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 16);
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}
