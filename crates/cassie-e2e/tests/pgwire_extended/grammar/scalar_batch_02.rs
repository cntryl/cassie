//! PostgreSQL 18.6 scalar expression descriptors and complete wire payloads.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_match_case_0_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 2^3^2 AS output";
    // Act
    let batches = planning::pruned("ten-batch-2-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 54, 52]],
            &[&[0, 1, 0, 0, 0, 8, 64, 80, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_1_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT -2^2 AS output";
    // Act
    let batches = planning::pruned("ten-batch-2-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 52]],
            &[&[0, 1, 0, 0, 0, 8, 64, 16, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_2_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT -(2^2) AS output";
    // Act
    let batches = planning::pruned("ten-batch-2-2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 52]],
            &[&[0, 1, 0, 0, 0, 8, 192, 16, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_3_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 2*3^2 AS output";
    // Act
    let batches = planning::pruned("ten-batch-2-3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 56]],
            &[&[0, 1, 0, 0, 0, 8, 64, 50, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_4_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT (2*3)^2 AS output";
    // Act
    let batches = planning::pruned("ten-batch-2-4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 51, 54]],
            &[&[0, 1, 0, 0, 0, 8, 64, 66, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_5_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 2^3+1 AS output";
    // Act
    let batches = planning::pruned("ten-batch-2-5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 57]],
            &[&[0, 1, 0, 0, 0, 8, 64, 34, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_6_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 2^(-2) AS output";
    // Act
    let batches = planning::pruned("ten-batch-2-6", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 48, 46, 50, 53]],
            &[&[0, 1, 0, 0, 0, 8, 63, 208, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_7_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 0^0 AS output";
    // Act
    let batches = planning::pruned("ten-batch-2-7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 8, 63, 240, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_8_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT -2^3 AS output";
    // Act
    let batches = planning::pruned("ten-batch-2-8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 56]],
            &[&[0, 1, 0, 0, 0, 8, 192, 32, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_9_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT NULL::FLOAT8^2 AS output";
    // Act
    let batches = planning::pruned("ten-batch-2-9", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_10_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 1.5::FLOAT8^2 AS output";
    // Act
    let batches = planning::pruned("ten-batch-2-10", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 50, 46, 50, 53]],
            &[&[0, 1, 0, 0, 0, 8, 64, 2, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_11_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 10%3 AS output";
    // Act
    let batches = planning::pruned("ten-batch-2-11", sql);
    // Assert
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
fn should_match_case_12_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT -10%3 AS output";
    // Act
    let batches = planning::pruned("ten-batch-2-12", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 49]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_13_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 10 % -3 AS output";
    // Act
    let batches = planning::pruned("ten-batch-2-13", sql);
    // Assert
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
fn should_match_case_14_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT (-9223372036854775808)::BIGINT % (-1)::BIGINT AS output";
    // Act
    let batches = planning::pruned("ten-batch-2-14", sql);
    // Assert
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
fn should_match_case_15_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT NULL::INTEGER%2 AS output";
    // Act
    let batches = planning::pruned("ten-batch-2-15", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_16_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT (7::SMALLINT)%(3::SMALLINT) AS output";
    // Act
    let batches = planning::pruned("ten-batch-2-16", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [&[&[0, 1, 0, 0, 0, 1, 49]], &[&[0, 1, 0, 0, 0, 2, 0, 1]]],
    );
}

#[test]
fn should_match_case_17_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT (7::BIGINT)%3 AS output";
    // Act
    let batches = planning::pruned("ten-batch-2-17", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_match_case_18_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT power(2::FLOAT8,3::FLOAT8) AS output";
    // Act
    let batches = planning::pruned("ten-batch-2-18", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 8, 64, 32, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_19_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT (2::FLOAT8)^(-3::INTEGER) AS output";
    // Act
    let batches = planning::pruned("ten-batch-2-19", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 48, 46, 49, 50, 53]],
            &[&[0, 1, 0, 0, 0, 8, 63, 192, 0, 0, 0, 0, 0, 0]],
        ],
    );
}
