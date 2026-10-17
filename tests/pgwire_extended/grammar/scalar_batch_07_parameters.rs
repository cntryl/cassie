//! Native declared and inferred cast-input identities across all wire formats.
use crate::support_values_casts;

#[test]
fn should_evaluate_int4_text_given_type_function_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT int4($1) AS output";
    // Act
    let batches =
        support_values_casts::parameters("type-function-int4_text", sql, 25, &[55], &[55]);
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_evaluate_float8_text_given_type_function_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT float8($1) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "type-function-float8_text",
        sql,
        25,
        &[49, 46, 53],
        &[49, 46, 53],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
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
fn should_evaluate_bool_text_given_type_function_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT bool($1) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "type-function-bool_text",
        sql,
        25,
        &[121, 101, 115],
        &[121, 101, 115],
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
fn should_evaluate_date_text_given_type_function_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT date($1) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "type-function-date_text",
        sql,
        25,
        &[50, 48, 48, 49, 45, 48, 50, 45, 49, 54],
        &[50, 48, 48, 49, 45, 48, 50, 45, 49, 54],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 1082, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 50, 48, 48, 49, 45, 48, 50, 45, 49, 54]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 1, 156]],
        ],
    );
}

#[test]
fn should_evaluate_uuid_text_given_type_function_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT uuid($1) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "type-function-uuid_text",
        sql,
        25,
        &[
            97, 48, 101, 101, 98, 99, 57, 57, 45, 57, 99, 48, 98, 45, 52, 101, 102, 56, 45, 98, 98,
            54, 100, 45, 54, 98, 98, 57, 98, 100, 51, 56, 48, 97, 49, 49,
        ],
        &[
            97, 48, 101, 101, 98, 99, 57, 57, 45, 57, 99, 48, 98, 45, 52, 101, 102, 56, 45, 98, 98,
            54, 100, 45, 54, 98, 98, 57, 98, 100, 51, 56, 48, 97, 49, 49,
        ],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 2950, 16, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 36, 97, 48, 101, 101, 98, 99, 57, 57, 45, 57, 99, 48, 98, 45, 52,
                101, 102, 56, 45, 98, 98, 54, 100, 45, 54, 98, 98, 57, 98, 100, 51, 56, 48, 97, 49,
                49,
            ]],
            &[&[
                0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185, 189,
                56, 10, 17,
            ]],
        ],
    );
}

#[test]
fn should_evaluate_bytea_text_given_type_function_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT bytea($1) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "type-function-bytea_text",
        sql,
        25,
        &[92, 120, 48, 48, 48, 49, 102, 102],
        &[92, 120, 48, 48, 48, 49, 102, 102],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 48, 49, 102, 102]],
            &[&[0, 1, 0, 0, 0, 3, 0, 1, 255]],
        ],
    );
}

#[test]
fn should_evaluate_int4_inferred_text_given_type_function_when_input_and_output_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT int4($1::TEXT) AS output";
    // Act
    let batches =
        support_values_casts::parameters("type-function-int4_inferred_text", sql, 0, &[55], &[55]);
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_evaluate_float8_inferred_text_given_type_function_when_input_and_output_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT float8($1::TEXT) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "type-function-float8_inferred_text",
        sql,
        0,
        &[49, 46, 53],
        &[49, 46, 53],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
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
fn should_evaluate_bool_inferred_text_given_type_function_when_input_and_output_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT bool($1::TEXT) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "type-function-bool_inferred_text",
        sql,
        0,
        &[121, 101, 115],
        &[121, 101, 115],
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
fn should_evaluate_date_inferred_text_given_type_function_when_input_and_output_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT date($1::TEXT) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "type-function-date_inferred_text",
        sql,
        0,
        &[50, 48, 48, 49, 45, 48, 50, 45, 49, 54],
        &[50, 48, 48, 49, 45, 48, 50, 45, 49, 54],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 1082, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 50, 48, 48, 49, 45, 48, 50, 45, 49, 54]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 1, 156]],
        ],
    );
}

#[test]
fn should_evaluate_uuid_inferred_text_given_type_function_when_input_and_output_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT uuid($1::TEXT) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "type-function-uuid_inferred_text",
        sql,
        0,
        &[
            97, 48, 101, 101, 98, 99, 57, 57, 45, 57, 99, 48, 98, 45, 52, 101, 102, 56, 45, 98, 98,
            54, 100, 45, 54, 98, 98, 57, 98, 100, 51, 56, 48, 97, 49, 49,
        ],
        &[
            97, 48, 101, 101, 98, 99, 57, 57, 45, 57, 99, 48, 98, 45, 52, 101, 102, 56, 45, 98, 98,
            54, 100, 45, 54, 98, 98, 57, 98, 100, 51, 56, 48, 97, 49, 49,
        ],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 2950, 16, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 36, 97, 48, 101, 101, 98, 99, 57, 57, 45, 57, 99, 48, 98, 45, 52,
                101, 102, 56, 45, 98, 98, 54, 100, 45, 54, 98, 98, 57, 98, 100, 51, 56, 48, 97, 49,
                49,
            ]],
            &[&[
                0, 1, 0, 0, 0, 16, 160, 238, 188, 153, 156, 11, 78, 248, 187, 109, 107, 185, 189,
                56, 10, 17,
            ]],
        ],
    );
}

#[test]
fn should_evaluate_bytea_inferred_text_given_type_function_when_input_and_output_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT bytea($1::TEXT) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "type-function-bytea_inferred_text",
        sql,
        0,
        &[92, 120, 48, 48, 48, 49, 102, 102],
        &[92, 120, 48, 48, 48, 49, 102, 102],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 48, 49, 102, 102]],
            &[&[0, 1, 0, 0, 0, 3, 0, 1, 255]],
        ],
    );
}

#[test]
fn should_evaluate_text_inferred_given_type_function_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT text($1) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "type-function-text_inferred",
        sql,
        0,
        &[104, 101, 108, 108, 111],
        &[104, 101, 108, 108, 111],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 104, 101, 108, 108, 111]],
            &[&[0, 1, 0, 0, 0, 5, 104, 101, 108, 108, 111]],
        ],
    );
}

#[test]
fn should_evaluate_varchar_inferred_given_type_function_when_input_and_output_formats_are_crossed()
{
    // Arrange
    let sql = "SELECT pg_catalog.varchar($1) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "type-function-varchar_inferred",
        sql,
        0,
        &[104, 101, 108, 108, 111],
        &[104, 101, 108, 108, 111],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1043);
    support_values_casts::assert_result(
        &batches,
        ("output", 1043, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 104, 101, 108, 108, 111]],
            &[&[0, 1, 0, 0, 0, 5, 104, 101, 108, 108, 111]],
        ],
    );
}

#[test]
fn should_evaluate_text_int4_given_type_function_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT text($1) AS output";
    // Act
    let batches =
        support_values_casts::parameters("type-function-text_int4", sql, 23, &[55], &[0, 0, 0, 7]);
    // Assert
    support_values_casts::assert_parameter(&batches, 23);
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 55]], &[&[0, 1, 0, 0, 0, 1, 55]]],
    );
}

#[test]
fn should_evaluate_int4_int2_given_type_function_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT int4($1) AS output";
    // Act
    let batches =
        support_values_casts::parameters("type-function-int4_int2", sql, 21, &[55], &[0, 7]);
    // Assert
    support_values_casts::assert_parameter(&batches, 21);
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}
