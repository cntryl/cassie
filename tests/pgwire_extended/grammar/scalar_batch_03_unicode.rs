//! PostgreSQL 18.6 scalar expression descriptors and complete wire payloads.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_match_case_0_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT U&'d!0061t' UESCAPE '!'||'abase' AS output";
    // Act
    let batches = planning::pruned("ten-batch-3-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 100, 97, 116, 97, 98, 97, 115, 101]],
            &[&[0, 1, 0, 0, 0, 8, 100, 97, 116, 97, 98, 97, 115, 101]],
        ],
    );
}

#[test]
fn should_match_case_1_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'x'||U&'\\0061' AS output";
    // Act
    let batches = planning::pruned("ten-batch-3-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 120, 97]],
            &[&[0, 1, 0, 0, 0, 2, 120, 97]],
        ],
    );
}

#[test]
fn should_match_case_2_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT (U&'0!0031' UESCAPE '!')::INTEGER & 3 AS output";
    // Act
    let batches = planning::pruned("ten-batch-3-2", sql);
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
