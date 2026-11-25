//! Native parameter identities and payloads across all four format combinations.
use crate::support_values_casts;

#[test]
fn should_measure_smallint_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size($1::SMALLINT) AS output";
    // Act
    let batches =
        support_values_casts::parameters("datum-param-smallint", sql, 21, &[45, 50], &[255, 254]);
    // Assert
    support_values_casts::assert_parameter(&batches, 21);
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 50]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 2]],
        ],
    );
}

#[test]
fn should_measure_int_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size($1::INTEGER) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "datum-param-int",
        sql,
        23,
        &[50, 49, 52, 55, 52, 56, 51, 54, 52, 55],
        &[127, 255, 255, 255],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 23);
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 52]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 4]],
        ],
    );
}

#[test]
fn should_measure_bigint_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size($1::BIGINT) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "datum-param-bigint",
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
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_measure_float_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size($1::FLOAT) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "datum-param-float",
        sql,
        701,
        &[49, 46, 53],
        &[63, 248, 0, 0, 0, 0, 0, 0],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 701);
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_measure_bool_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size($1::BOOLEAN) AS output";
    // Act
    let batches =
        support_values_casts::parameters("datum-param-bool", sql, 16, &[116, 114, 117, 101], &[1]);
    // Assert
    support_values_casts::assert_parameter(&batches, 16);
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_measure_date_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size($1::DATE) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "datum-param-date",
        sql,
        1082,
        &[50, 48, 48, 48, 45, 48, 49, 45, 48, 49],
        &[0, 0, 0, 0],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1082);
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 52]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 4]],
        ],
    );
}

#[test]
fn should_measure_time_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size($1::TIME) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "datum-param-time",
        sql,
        1083,
        &[48, 48, 58, 48, 48, 58, 48, 48, 46, 48, 48, 48, 48, 48, 49],
        &[0, 0, 0, 0, 0, 0, 0, 1],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1083);
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_measure_timestamp_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size($1::TIMESTAMP) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "datum-param-timestamp",
        sql,
        1114,
        &[
            50, 48, 48, 48, 45, 48, 49, 45, 48, 49, 32, 48, 48, 58, 48, 48, 58, 48, 48, 46, 48, 48,
            48, 48, 48, 49,
        ],
        &[0, 0, 0, 0, 0, 0, 0, 1],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1114);
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_measure_uuid_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size($1::UUID) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "datum-param-uuid",
        sql,
        2950,
        &[
            48, 48, 48, 48, 48, 48, 48, 48, 45, 48, 48, 48, 48, 45, 48, 48, 48, 48, 45, 48, 48, 48,
            48, 45, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 49,
        ],
        &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 2950);
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 54]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 16]],
        ],
    );
}

#[test]
fn should_measure_text_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size($1::TEXT) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "datum-param-text",
        sql,
        25,
        &[195, 169, 240, 159, 153, 130],
        &[195, 169, 240, 159, 153, 130],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 10]],
        ],
    );
}

#[test]
fn should_measure_varchar_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size($1::VARCHAR(3)) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "datum-param-varchar",
        sql,
        1043,
        &[97, 98, 99, 100, 101, 102],
        &[97, 98, 99, 100, 101, 102],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1043);
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
fn should_measure_bytea_given_explicit_parameters_when_input_and_output_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size($1::BYTEA) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "datum-param-bytea",
        sql,
        17,
        &[92, 120, 48, 48, 97, 98, 102, 102],
        &[0, 171, 255],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 17);
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}
