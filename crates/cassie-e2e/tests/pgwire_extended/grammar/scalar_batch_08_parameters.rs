//! Native declared and inferred cast-input identities across all wire formats.
use crate::support_values_casts;

#[test]
fn should_evaluate_value_unknown_given_like_escape_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1 LIKE 'a!_b' ESCAPE '!' AS output";
    // Act
    let batches = support_values_casts::parameters(
        "like-escape-value_unknown",
        sql,
        0,
        &[97, 95, 98],
        &[97, 95, 98],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_evaluate_pattern_unknown_given_like_escape_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT 'a_b' LIKE $1 ESCAPE '!' AS output";
    // Act
    let batches = support_values_casts::parameters(
        "like-escape-pattern_unknown",
        sql,
        0,
        &[97, 33, 95, 98],
        &[97, 33, 95, 98],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_evaluate_escape_unknown_given_like_escape_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT 'a_b' LIKE 'a!_b' ESCAPE $1 AS output";
    // Act
    let batches =
        support_values_casts::parameters("like-escape-escape_unknown", sql, 0, &[33], &[33]);
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_evaluate_value_text_given_like_escape_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1 LIKE 'a!_b' ESCAPE '!' AS output";
    // Act
    let batches = support_values_casts::parameters(
        "like-escape-value_text",
        sql,
        25,
        &[97, 95, 98],
        &[97, 95, 98],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_evaluate_escape_varchar_given_like_escape_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT 'a_b' LIKE 'a!_b' ESCAPE $1 AS output";
    // Act
    let batches =
        support_values_casts::parameters("like-escape-escape_varchar", sql, 1043, &[33], &[33]);
    // Assert
    support_values_casts::assert_parameter(&batches, 1043);
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_evaluate_escape_char_given_like_escape_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT 'a_b' LIKE 'a!_b' ESCAPE $1 AS output";
    // Act
    let batches = support_values_casts::parameters(
        "like-escape-escape_char",
        sql,
        1042,
        &[33, 32],
        &[33, 32],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1042);
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}
