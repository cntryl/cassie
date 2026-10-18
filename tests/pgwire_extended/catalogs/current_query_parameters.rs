//! Native integer parameter identity is independent of current statement text.
use crate::support_values_casts;

#[test]
fn should_preserve_text_0_given_integer_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT current_query() AS output WHERE $1::INT = 1";
    // Act
    let batches =
        support_values_casts::parameters("query-parameter-0", sql, 23, b"1", &[0, 0, 0, 1]);
    // Assert
    support_values_casts::assert_parameter(&batches, 23);
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 50, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110, 116,
                95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116,
                32, 87, 72, 69, 82, 69, 32, 36, 49, 58, 58, 73, 78, 84, 32, 61, 32, 49,
            ]],
            &[&[
                0, 1, 0, 0, 0, 50, 83, 69, 76, 69, 67, 84, 32, 99, 117, 114, 114, 101, 110, 116,
                95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83, 32, 111, 117, 116, 112, 117, 116,
                32, 87, 72, 69, 82, 69, 32, 36, 49, 58, 58, 73, 78, 84, 32, 61, 32, 49,
            ]],
        ],
    );
}

#[test]
fn should_preserve_text_1_given_integer_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "/* bound α */ SELECT pg_catalog.current_query() AS output WHERE $1::INT = 1";
    // Act
    let batches =
        support_values_casts::parameters("query-parameter-1", sql, 23, b"1", &[0, 0, 0, 1]);
    // Assert
    support_values_casts::assert_parameter(&batches, 23);
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 76, 47, 42, 32, 98, 111, 117, 110, 100, 32, 206, 177, 32, 42, 47,
                32, 83, 69, 76, 69, 67, 84, 32, 112, 103, 95, 99, 97, 116, 97, 108, 111, 103, 46,
                99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83,
                32, 111, 117, 116, 112, 117, 116, 32, 87, 72, 69, 82, 69, 32, 36, 49, 58, 58, 73,
                78, 84, 32, 61, 32, 49,
            ]],
            &[&[
                0, 1, 0, 0, 0, 76, 47, 42, 32, 98, 111, 117, 110, 100, 32, 206, 177, 32, 42, 47,
                32, 83, 69, 76, 69, 67, 84, 32, 112, 103, 95, 99, 97, 116, 97, 108, 111, 103, 46,
                99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 32, 65, 83,
                32, 111, 117, 116, 112, 117, 116, 32, 87, 72, 69, 82, 69, 32, 36, 49, 58, 58, 73,
                78, 84, 32, 61, 32, 49,
            ]],
        ],
    );
}

#[test]
fn should_preserve_text_2_given_integer_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "WITH q AS (SELECT current_query() AS output WHERE $1::INT = 1) SELECT output FROM q";
    // Act
    let batches =
        support_values_casts::parameters("query-parameter-2", sql, 23, b"1", &[0, 0, 0, 1]);
    // Assert
    support_values_casts::assert_parameter(&batches, 23);
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 83, 87, 73, 84, 72, 32, 113, 32, 65, 83, 32, 40, 83, 69, 76, 69, 67,
                84, 32, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 32,
                65, 83, 32, 111, 117, 116, 112, 117, 116, 32, 87, 72, 69, 82, 69, 32, 36, 49, 58,
                58, 73, 78, 84, 32, 61, 32, 49, 41, 32, 83, 69, 76, 69, 67, 84, 32, 111, 117, 116,
                112, 117, 116, 32, 70, 82, 79, 77, 32, 113,
            ]],
            &[&[
                0, 1, 0, 0, 0, 83, 87, 73, 84, 72, 32, 113, 32, 65, 83, 32, 40, 83, 69, 76, 69, 67,
                84, 32, 99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 32,
                65, 83, 32, 111, 117, 116, 112, 117, 116, 32, 87, 72, 69, 82, 69, 32, 36, 49, 58,
                58, 73, 78, 84, 32, 61, 32, 49, 41, 32, 83, 69, 76, 69, 67, 84, 32, 111, 117, 116,
                112, 117, 116, 32, 70, 82, 79, 77, 32, 113,
            ]],
        ],
    );
}

#[test]
fn should_preserve_text_3_given_integer_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT lower(current_query()) AS output WHERE $1::INT = 1";
    // Act
    let batches =
        support_values_casts::parameters("query-parameter-3", sql, 23, b"1", &[0, 0, 0, 1]);
    // Assert
    support_values_casts::assert_parameter(&batches, 23);
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 57, 115, 101, 108, 101, 99, 116, 32, 108, 111, 119, 101, 114, 40,
                99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 41, 32, 97,
                115, 32, 111, 117, 116, 112, 117, 116, 32, 119, 104, 101, 114, 101, 32, 36, 49, 58,
                58, 105, 110, 116, 32, 61, 32, 49,
            ]],
            &[&[
                0, 1, 0, 0, 0, 57, 115, 101, 108, 101, 99, 116, 32, 108, 111, 119, 101, 114, 40,
                99, 117, 114, 114, 101, 110, 116, 95, 113, 117, 101, 114, 121, 40, 41, 41, 32, 97,
                115, 32, 111, 117, 116, 112, 117, 116, 32, 119, 104, 101, 114, 101, 32, 36, 49, 58,
                58, 105, 110, 116, 32, 61, 32, 49,
            ]],
        ],
    );
}
