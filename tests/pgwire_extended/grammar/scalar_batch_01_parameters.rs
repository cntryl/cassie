//! Native parameter identities and payloads across all four format combinations.
use crate::support_values_casts;

#[test]
fn should_concatenate_int_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1||'x' AS output";
    // Act
    let batches =
        support_values_casts::parameters("concat-param-int", sql, 23, &[55], &[0, 0, 0, 7]);
    // Assert
    support_values_casts::assert_parameter(&batches, 23);
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 55, 120]],
            &[&[0, 1, 0, 0, 0, 2, 55, 120]],
        ],
    );
}

#[test]
fn should_concatenate_bigint_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1||'x' AS output";
    // Act
    let batches = support_values_casts::parameters(
        "concat-param-bigint",
        sql,
        20,
        &[
            57, 48, 48, 55, 49, 57, 57, 50, 53, 52, 55, 52, 48, 57, 57, 51,
        ],
        &[0, 32, 0, 0, 0, 0, 0, 1],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 20);
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 17, 57, 48, 48, 55, 49, 57, 57, 50, 53, 52, 55, 52, 48, 57, 57, 51,
                120,
            ]],
            &[&[
                0, 1, 0, 0, 0, 17, 57, 48, 48, 55, 49, 57, 57, 50, 53, 52, 55, 52, 48, 57, 57, 51,
                120,
            ]],
        ],
    );
}

#[test]
fn should_concatenate_text_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1||'x' AS output";
    // Act
    let batches = support_values_casts::parameters(
        "concat-param-text",
        sql,
        25,
        &[195, 169, 240, 159, 153, 130],
        &[195, 169, 240, 159, 153, 130],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 7, 195, 169, 240, 159, 153, 130, 120]],
            &[&[0, 1, 0, 0, 0, 7, 195, 169, 240, 159, 153, 130, 120]],
        ],
    );
}

#[test]
fn should_concatenate_bool_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1||'x' AS output";
    // Act
    let batches = support_values_casts::parameters(
        "concat-param-bool",
        sql,
        16,
        &[102, 97, 108, 115, 101],
        &[0],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 16);
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 102, 97, 108, 115, 101, 120]],
            &[&[0, 1, 0, 0, 0, 6, 102, 97, 108, 115, 101, 120]],
        ],
    );
}
