//! PostgreSQL 18.6 scalar expression descriptors and complete wire payloads.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_match_case_0_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 2 BETWEEN SYMMETRIC 3 AND 1 AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_1_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 2 NOT BETWEEN SYMMETRIC 3 AND 1 AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_2_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 4 BETWEEN SYMMETRIC 3 AND 1 AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_3_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 4 NOT BETWEEN SYMMETRIC 3 AND 1 AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_4_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 1 BETWEEN SYMMETRIC 1 AND 3 AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_5_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 1 NOT BETWEEN SYMMETRIC 1 AND 3 AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_6_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 3 BETWEEN SYMMETRIC 3 AND 1 AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-6", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_7_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 3 NOT BETWEEN SYMMETRIC 3 AND 1 AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_8_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 2 BETWEEN SYMMETRIC 2 AND 2 AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_9_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 2 NOT BETWEEN SYMMETRIC 2 AND 2 AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-9", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_10_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT NULL::INT BETWEEN SYMMETRIC 3 AND 1 AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-10", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_11_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT NULL::INT NOT BETWEEN SYMMETRIC 3 AND 1 AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-11", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_12_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 2 BETWEEN SYMMETRIC NULL::INT AND 1 AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-12", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_13_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 2 NOT BETWEEN SYMMETRIC NULL::INT AND 1 AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-13", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_14_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 2 BETWEEN SYMMETRIC 3 AND NULL::INT AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-14", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_15_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 2 NOT BETWEEN SYMMETRIC 3 AND NULL::INT AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-15", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_16_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT NULL::INT BETWEEN SYMMETRIC NULL::INT AND NULL::INT AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-16", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_17_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT NULL::INT NOT BETWEEN SYMMETRIC NULL::INT AND NULL::INT AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-17", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_18_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 2 BETWEEN ASYMMETRIC 3 AND 1 AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-18", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_19_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 2 NOT BETWEEN ASYMMETRIC 3 AND 1 AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-19", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_20_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 2 BETWEEN SYMMETRIC 3 AND 1 AND FALSE AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-20", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_21_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT NOT 2 BETWEEN SYMMETRIC 3 AND 1 AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-21", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_22_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT (FALSE AND 1/0=0) BETWEEN SYMMETRIC FALSE AND TRUE AS output";
    // Act
    let batches = planning::pruned("ten-batch-6-22", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}
