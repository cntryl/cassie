//! PostgreSQL 18.6 scalar expression descriptors and complete wire payloads.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_match_case_0_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'a'||'b' AS output";
    // Act
    let batches = planning::pruned("ten-batch-1-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 97, 98]],
            &[&[0, 1, 0, 0, 0, 2, 97, 98]],
        ],
    );
}

#[test]
fn should_match_case_1_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'é'||'🙂' AS output";
    // Act
    let batches = planning::pruned("ten-batch-1-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 195, 169, 240, 159, 153, 130]],
            &[&[0, 1, 0, 0, 0, 6, 195, 169, 240, 159, 153, 130]],
        ],
    );
}

#[test]
fn should_match_case_2_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT NULL::TEXT||'x' AS output";
    // Act
    let batches = planning::pruned("ten-batch-1-2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_3_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'x'||NULL::TEXT AS output";
    // Act
    let batches = planning::pruned("ten-batch-1-3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_4_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'a'||1+2 AS output";
    // Act
    let batches = planning::pruned("ten-batch-1-4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 97, 51]],
            &[&[0, 1, 0, 0, 0, 2, 97, 51]],
        ],
    );
}

#[test]
fn should_match_case_5_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 1+2||'b' AS output";
    // Act
    let batches = planning::pruned("ten-batch-1-5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 51, 98]],
            &[&[0, 1, 0, 0, 0, 2, 51, 98]],
        ],
    );
}

#[test]
fn should_match_case_6_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'x'||true AS output";
    // Act
    let batches = planning::pruned("ten-batch-1-6", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 5, 120, 116, 114, 117, 101]],
            &[&[0, 1, 0, 0, 0, 5, 120, 116, 114, 117, 101]],
        ],
    );
}

#[test]
fn should_match_case_7_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT false||'x' AS output";
    // Act
    let batches = planning::pruned("ten-batch-1-7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 6, 102, 97, 108, 115, 101, 120]],
            &[&[0, 1, 0, 0, 0, 6, 102, 97, 108, 115, 101, 120]],
        ],
    );
}

#[test]
fn should_match_case_8_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT '('||trim(' x ')||')' AS output";
    // Act
    let batches = planning::pruned("ten-batch-1-8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 40, 120, 41]],
            &[&[0, 1, 0, 0, 0, 3, 40, 120, 41]],
        ],
    );
}

#[test]
fn should_match_case_9_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'a'||'b'||'c' AS output";
    // Act
    let batches = planning::pruned("ten-batch-1-9", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 97, 98, 99]],
            &[&[0, 1, 0, 0, 0, 3, 97, 98, 99]],
        ],
    );
}

#[test]
fn should_match_case_10_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT CAST('x' AS VARCHAR(3))||'y' AS output";
    // Act
    let batches = planning::pruned("ten-batch-1-10", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 120, 121]],
            &[&[0, 1, 0, 0, 0, 2, 120, 121]],
        ],
    );
}

#[test]
fn should_match_case_11_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT ('a'||'b')='ab' AS output";
    // Act
    let batches = planning::pruned("ten-batch-1-11", sql);
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
    let sql = "SELECT 'a'||(1+2)*3 AS output";
    // Act
    let batches = planning::pruned("ten-batch-1-12", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 97, 57]],
            &[&[0, 1, 0, 0, 0, 2, 97, 57]],
        ],
    );
}

#[test]
fn should_match_case_13_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT ('x'||NULL::TEXT) IS NULL AS output";
    // Act
    let batches = planning::pruned("ten-batch-1-13", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_14_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'a || b'||'c' AS output";
    // Act
    let batches = planning::pruned("ten-batch-1-14", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 7, 97, 32, 124, 124, 32, 98, 99]],
            &[&[0, 1, 0, 0, 0, 7, 97, 32, 124, 124, 32, 98, 99]],
        ],
    );
}
