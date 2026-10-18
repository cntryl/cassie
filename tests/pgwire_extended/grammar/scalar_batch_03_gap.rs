//! PostgreSQL 18.6 scalar expression descriptors and complete wire payloads.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_match_case_0_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT (-32768)::SMALLINT << (-33) AS output";
    // Act
    let batches = planning::pruned("ten-batch-3-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [&[&[0, 1, 0, 0, 0, 1, 48]], &[&[0, 1, 0, 0, 0, 2, 0, 0]]],
    );
}

#[test]
fn should_match_case_1_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT (-32768)::SMALLINT >> (-33) AS output";
    // Act
    let batches = planning::pruned("ten-batch-3-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 49]],
            &[&[0, 1, 0, 0, 0, 2, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_2_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT (-9223372036854775808)::BIGINT >> 2147483647 AS output";
    // Act
    let batches = planning::pruned("ten-batch-3-2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 49]],
            &[&[0, 1, 0, 0, 0, 8, 255, 255, 255, 255, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_3_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 2147483647::INTEGER << (-32) AS output";
    // Act
    let batches = planning::pruned("ten-batch-3-3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 50, 49, 52, 55, 52, 56, 51, 54, 52, 55]],
            &[&[0, 1, 0, 0, 0, 4, 127, 255, 255, 255]],
        ],
    );
}
