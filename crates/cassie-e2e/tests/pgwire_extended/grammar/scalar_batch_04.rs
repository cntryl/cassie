//! PostgreSQL 18.6 scalar expression descriptors and complete wire payloads.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_match_case_0_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT |/25.0 AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 53]],
            &[&[0, 1, 0, 0, 0, 8, 64, 20, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_1_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT ||/64.0 AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-1", sql);
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
    let sql = "SELECT @ -7 AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-2", sql);
    // Assert
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
fn should_match_case_3_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT |/25 AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 53]],
            &[&[0, 1, 0, 0, 0, 8, 64, 20, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_4_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT ||/(-8) AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 50]],
            &[&[0, 1, 0, 0, 0, 8, 192, 0, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_5_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT |/(-0.0)::FLOAT8 AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 48]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_6_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT ||/(-0.0)::FLOAT8 AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-6", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 48]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_7_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT |/'Infinity'::FLOAT8 AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 73, 110, 102, 105, 110, 105, 116, 121]],
            &[&[0, 1, 0, 0, 0, 8, 127, 240, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_8_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT ||/'-Infinity'::FLOAT8 AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 9, 45, 73, 110, 102, 105, 110, 105, 116, 121]],
            &[&[0, 1, 0, 0, 0, 8, 255, 240, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_9_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT |/'NaN'::FLOAT8 AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-9", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 78, 97, 78]],
            &[&[0, 1, 0, 0, 0, 8, 127, 248, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_10_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT @ (-32767)::SMALLINT AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-10", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 51, 50, 55, 54, 55]],
            &[&[0, 1, 0, 0, 0, 2, 127, 255]],
        ],
    );
}

#[test]
fn should_match_case_11_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT @ (-2147483647)::INT AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-11", sql);
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

#[test]
fn should_match_case_12_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT @ (-9223372036854775807)::BIGINT AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-12", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 19, 57, 50, 50, 51, 51, 55, 50, 48, 51, 54, 56, 53, 52, 55, 55, 53,
                56, 48, 55,
            ]],
            &[&[0, 1, 0, 0, 0, 8, 127, 255, 255, 255, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_13_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT @ (-1.5)::FLOAT8 AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-13", sql);
    // Assert
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
fn should_match_case_14_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT @ '-3' AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-14", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 51]],
            &[&[0, 1, 0, 0, 0, 8, 64, 8, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_15_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT |/NULL AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-15", sql);
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
fn should_match_case_16_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT ||/NULL AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-16", sql);
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
fn should_match_case_17_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT @ NULL AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-17", sql);
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
fn should_match_case_18_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT @ NULL::SMALLINT AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-18", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_19_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT |/ (9::BIGINT) AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-19", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 51]],
            &[&[0, 1, 0, 0, 0, 8, 64, 8, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_20_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT |/9+7 AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-20", sql);
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
fn should_match_case_21_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT ||/8*8 AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-21", sql);
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
fn should_match_case_22_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT @ -7+10 AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-22", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 51]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 3]],
        ],
    );
}

#[test]
fn should_match_case_23_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT |/9^2 AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-23", sql);
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
fn should_match_case_24_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT ~1+2 AS output";
    // Act
    let batches = planning::pruned("ten-batch-4-24", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 52]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 252]],
        ],
    );
}
