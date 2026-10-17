//! PostgreSQL 18.6 scalar expression descriptors and complete wire payloads.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_match_case_0_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT TRUE IS TRUE AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-0", sql);
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
    let sql = "SELECT TRUE IS FALSE AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-1", sql);
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
    let sql = "SELECT TRUE IS UNKNOWN AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-2", sql);
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
    let sql = "SELECT TRUE IS NOT TRUE AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_4_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT TRUE IS NOT FALSE AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-4", sql);
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
    let sql = "SELECT TRUE IS NOT UNKNOWN AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_6_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT FALSE IS TRUE AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-6", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_7_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT FALSE IS FALSE AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_8_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT FALSE IS UNKNOWN AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_9_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT FALSE IS NOT TRUE AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-9", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_10_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT FALSE IS NOT FALSE AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-10", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_11_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT FALSE IS NOT UNKNOWN AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-11", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_12_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT NULL IS TRUE AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-12", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_13_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT NULL IS FALSE AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-13", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_14_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT NULL IS UNKNOWN AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-14", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_15_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT NULL IS NOT TRUE AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-15", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_16_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT NULL IS NOT FALSE AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-16", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_17_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT NULL IS NOT UNKNOWN AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-17", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_18_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'yes' IS TRUE AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-18", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_19_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'off' IS FALSE AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-19", sql);
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
    let sql = "SELECT '1' IS TRUE AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-20", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_21_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT '0' IS NOT TRUE AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-21", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_22_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 1=1 IS TRUE AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-22", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_23_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT NOT FALSE IS TRUE AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-23", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_24_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT TRUE IS FALSE OR TRUE AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-24", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_25_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT FALSE IS NOT UNKNOWN AND TRUE AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-25", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_26_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT (NULL IS UNKNOWN) IS TRUE AS output";
    // Act
    let batches = planning::pruned("ten-batch-5-26", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}
