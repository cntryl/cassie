//! Explicit primitive parameter inputs and exact native scalar output.
use crate::support_values_casts;

#[test]
fn should_preserve_encoding_given_explicit_parameters_when_input_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_char_to_encoding($1::text) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "catalog-param-encoding",
        sql,
        25,
        &[85, 84, 70, 45, 56],
        &[85, 84, 70, 45, 56],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 54]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 6]],
        ],
    );
}

#[test]
fn should_preserve_encoding_alias_given_explicit_parameters_when_input_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_char_to_encoding($1::text) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "catalog-param-encoding_alias",
        sql,
        25,
        &[119, 105, 110, 100, 111, 119, 115, 45, 49, 50, 53, 50],
        &[119, 105, 110, 100, 111, 119, 115, 45, 49, 50, 53, 50],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 50, 52]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 24]],
        ],
    );
}

#[test]
fn should_preserve_bytes_given_explicit_parameters_when_input_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_size_bytes($1::text) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "catalog-param-bytes",
        sql,
        25,
        &[49, 46, 53, 32, 77, 66],
        &[49, 46, 53, 32, 77, 66],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 7, 49, 53, 55, 50, 56, 54, 52]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 24, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_bytes_precise_given_explicit_parameters_when_input_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_size_bytes($1::text) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "catalog-param-bytes_precise",
        sql,
        25,
        &[
            48, 46, 48, 48, 48, 52, 56, 56, 50, 56, 49, 50, 52, 57, 57, 57, 57, 57, 57, 57, 57, 57,
            57, 57, 57, 57, 32, 107, 66,
        ],
        &[
            48, 46, 48, 48, 48, 52, 56, 56, 50, 56, 49, 50, 52, 57, 57, 57, 57, 57, 57, 57, 57, 57,
            57, 57, 57, 57, 32, 107, 66,
        ],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
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
fn should_preserve_pretty_given_explicit_parameters_when_input_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_size_pretty($1::bigint) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "catalog-param-pretty",
        sql,
        20,
        &[49, 48, 55, 53, 50],
        &[0, 0, 0, 0, 0, 0, 42, 0],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 20);
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 49, 49, 32, 107, 66]],
            &[&[0, 1, 0, 0, 0, 5, 49, 49, 32, 107, 66]],
        ],
    );
}

#[test]
fn should_preserve_pretty_min_given_explicit_parameters_when_input_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_size_pretty($1::bigint) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "catalog-param-pretty_min",
        sql,
        20,
        &[
            45, 57, 50, 50, 51, 51, 55, 50, 48, 51, 54, 56, 53, 52, 55, 55, 53, 56, 48, 56,
        ],
        &[128, 0, 0, 0, 0, 0, 0, 0],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 20);
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 45, 56, 49, 57, 50, 32, 80, 66]],
            &[&[0, 1, 0, 0, 0, 8, 45, 56, 49, 57, 50, 32, 80, 66]],
        ],
    );
}
