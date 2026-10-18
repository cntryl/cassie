//! Native VARCHAR typmod casts of retained parameters.
use crate::support_values_casts;

#[test]
fn should_truncate_text_1_given_supplied_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::VARCHAR(1) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "varchar-param-text_1",
        sql,
        25,
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 1043, -1, 5),
        [&[&[0, 1, 0, 0, 0, 1, 97]], &[&[0, 1, 0, 0, 0, 1, 97]]],
    );
}

#[test]
fn should_truncate_varchar_1_given_supplied_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::VARCHAR(1) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "varchar-param-varchar_1",
        sql,
        1043,
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1043);
    support_values_casts::assert_result(
        &batches,
        ("output", 1043, -1, 5),
        [&[&[0, 1, 0, 0, 0, 1, 97]], &[&[0, 1, 0, 0, 0, 1, 97]]],
    );
}

#[test]
fn should_truncate_bpchar_1_given_supplied_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::VARCHAR(1) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "varchar-param-bpchar_1",
        sql,
        1042,
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1042);
    support_values_casts::assert_result(
        &batches,
        ("output", 1043, -1, 5),
        [&[&[0, 1, 0, 0, 0, 1, 97]], &[&[0, 1, 0, 0, 0, 1, 97]]],
    );
}

#[test]
fn should_truncate_text_2_given_supplied_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::VARCHAR(2) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "varchar-param-text_2",
        sql,
        25,
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 1043, -1, 6),
        [
            &[&[0, 1, 0, 0, 0, 5, 97, 240, 159, 153, 130]],
            &[&[0, 1, 0, 0, 0, 5, 97, 240, 159, 153, 130]],
        ],
    );
}

#[test]
fn should_truncate_varchar_2_given_supplied_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::VARCHAR(2) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "varchar-param-varchar_2",
        sql,
        1043,
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1043);
    support_values_casts::assert_result(
        &batches,
        ("output", 1043, -1, 6),
        [
            &[&[0, 1, 0, 0, 0, 5, 97, 240, 159, 153, 130]],
            &[&[0, 1, 0, 0, 0, 5, 97, 240, 159, 153, 130]],
        ],
    );
}

#[test]
fn should_truncate_bpchar_2_given_supplied_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::VARCHAR(2) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "varchar-param-bpchar_2",
        sql,
        1042,
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1042);
    support_values_casts::assert_result(
        &batches,
        ("output", 1043, -1, 6),
        [
            &[&[0, 1, 0, 0, 0, 5, 97, 240, 159, 153, 130]],
            &[&[0, 1, 0, 0, 0, 5, 97, 240, 159, 153, 130]],
        ],
    );
}

#[test]
fn should_truncate_text_3_given_supplied_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::VARCHAR(3) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "varchar-param-text_3",
        sql,
        25,
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 1043, -1, 7),
        [
            &[&[0, 1, 0, 0, 0, 8, 97, 240, 159, 153, 130, 230, 150, 135]],
            &[&[0, 1, 0, 0, 0, 8, 97, 240, 159, 153, 130, 230, 150, 135]],
        ],
    );
}

#[test]
fn should_truncate_varchar_3_given_supplied_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::VARCHAR(3) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "varchar-param-varchar_3",
        sql,
        1043,
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1043);
    support_values_casts::assert_result(
        &batches,
        ("output", 1043, -1, 7),
        [
            &[&[0, 1, 0, 0, 0, 8, 97, 240, 159, 153, 130, 230, 150, 135]],
            &[&[0, 1, 0, 0, 0, 8, 97, 240, 159, 153, 130, 230, 150, 135]],
        ],
    );
}

#[test]
fn should_truncate_bpchar_3_given_supplied_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::VARCHAR(3) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "varchar-param-bpchar_3",
        sql,
        1042,
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1042);
    support_values_casts::assert_result(
        &batches,
        ("output", 1043, -1, 7),
        [
            &[&[0, 1, 0, 0, 0, 8, 97, 240, 159, 153, 130, 230, 150, 135]],
            &[&[0, 1, 0, 0, 0, 8, 97, 240, 159, 153, 130, 230, 150, 135]],
        ],
    );
}

#[test]
fn should_truncate_text_7_given_supplied_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::VARCHAR(7) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "varchar-param-text_7",
        sql,
        25,
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_values_casts::assert_result(
        &batches,
        ("output", 1043, -1, 11),
        [
            &[&[
                0, 1, 0, 0, 0, 12, 97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32,
            ]],
            &[&[
                0, 1, 0, 0, 0, 12, 97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32,
            ]],
        ],
    );
}

#[test]
fn should_truncate_varchar_7_given_supplied_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::VARCHAR(7) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "varchar-param-varchar_7",
        sql,
        1043,
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1043);
    support_values_casts::assert_result(
        &batches,
        ("output", 1043, -1, 11),
        [
            &[&[
                0, 1, 0, 0, 0, 12, 97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32,
            ]],
            &[&[
                0, 1, 0, 0, 0, 12, 97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32,
            ]],
        ],
    );
}

#[test]
fn should_truncate_bpchar_7_given_supplied_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT $1::VARCHAR(7) AS output";
    // Act
    let batches = support_values_casts::parameters(
        "varchar-param-bpchar_7",
        sql,
        1042,
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
        &[97, 240, 159, 153, 130, 230, 150, 135, 120, 121, 32, 32],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1042);
    support_values_casts::assert_result(
        &batches,
        ("output", 1043, -1, 11),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 97, 240, 159, 153, 130, 230, 150, 135, 120, 121,
            ]],
            &[&[
                0, 1, 0, 0, 0, 10, 97, 240, 159, 153, 130, 230, 150, 135, 120, 121,
            ]],
        ],
    );
}
