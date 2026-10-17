//! PostgreSQL 18.6 scalar expression descriptors and complete wire payloads.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_match_case_0_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'a_b' LIKE 'a!_b' ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-0", sql);
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
    let sql = "SELECT 'axb' LIKE 'a!_b' ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-1", sql);
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
    let sql = "SELECT 'a!xb' LIKE 'a!!_b' ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_3_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'a_b' LIKE 'a!_b' ESCAPE '' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-3", sql);
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
    let sql = "SELECT 'a_b' NOT LIKE 'a!_b' ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_5_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT NOT ('a_b' LIKE 'a!_b' ESCAPE '!') AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-5", sql);
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
    let sql = "SELECT 'a%b' LIKE 'a!%b' ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-6", sql);
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
    let sql = "SELECT 'axb' LIKE 'a!%b' ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-7", sql);
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
    let sql = "SELECT 'a%b' LIKE 'a%%b' ESCAPE '%' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-8", sql);
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
    let sql = "SELECT 'ab' LIKE 'a%b' ESCAPE '%' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-9", sql);
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
    let sql = "SELECT 'a_b' LIKE 'a__b' ESCAPE '_' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-10", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_11_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'a_b' LIKE 'aé_b' ESCAPE 'é' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-11", sql);
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
    let sql = "SELECT 'é' LIKE '_' ESCAPE '' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-12", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_13_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'é' LIKE '§_' ESCAPE '§' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-13", sql);
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
    let sql = "SELECT '_' LIKE '§_' ESCAPE '§' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-14", sql);
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
    let sql = "SELECT 'a\\b' LIKE 'a\\b' ESCAPE '' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-15", sql);
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
    let sql = "SELECT 'a\\b' LIKE 'a\\b' ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-16", sql);
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
    let sql = "SELECT '' LIKE '' ESCAPE '' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-17", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_18_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT '' LIKE '%' ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-18", sql);
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
    let sql = "SELECT 'a!b' LIKE 'a!!b' ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-19", sql);
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
    let sql = "SELECT NULL::TEXT LIKE 'x' ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-20", sql);
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
fn should_match_case_21_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'x' LIKE NULL::TEXT ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-21", sql);
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
fn should_match_case_22_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'x' LIKE 'x' ESCAPE NULL::TEXT AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-22", sql);
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
fn should_match_case_23_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT NULL::TEXT LIKE NULL::TEXT ESCAPE '!!' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-23", sql);
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
fn should_match_case_24_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'x' LIKE NULL::TEXT ESCAPE '!!' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-24", sql);
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
fn should_match_case_25_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'a_b' LIKE 'a!_b' ESCAPE '!' AND TRUE AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-25", sql);
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
    let sql = "SELECT FALSE OR 'a_b' LIKE 'a!_b' ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-26", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_27_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'x' LIKE 'y!' ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-27", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_28_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'x' LIKE 'y\\' ESCAPE '\\' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-28", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_29_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT '_' LIKE '__' ESCAPE '_' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-29", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_30_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT '%' LIKE '%%' ESCAPE '%' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-30", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_31_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'é' LIKE 'éé' ESCAPE 'é' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-31", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_32_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'a_b' LIKE 'a!_b' ESCAPE '!' = TRUE AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-32", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_33_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT TRUE = 'a_b' LIKE 'a!_b' ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-33", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_34_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'a_b' NOT LIKE 'a!_b' ESCAPE '!' = FALSE AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-34", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_35_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'a_b' LIKE 'a!_b' ESCAPE '!' IS TRUE AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-35", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_36_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'a_b' LIKE 'a!_b' ESCAPE CASE WHEN TRUE THEN '!' ELSE '#' END AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-36", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_37_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'a_b' LIKE CASE WHEN TRUE THEN 'a!_b' ELSE 'x' END ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-37", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_38_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT CASE WHEN 'a_b' LIKE 'a!_b' ESCAPE '!' THEN 7 ELSE 8 END AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-38", sql);
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
fn should_match_case_39_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'x' LIKE 'x!' ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-39", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_40_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT '' LIKE '!' ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-40", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_41_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT 'x' LIKE 'x\\' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-41", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_42_given_selected_expression_when_native_scalar_is_executed() {
    // Arrange
    let sql = "SELECT '' LIKE '%\\' AS output";
    // Act
    let batches = planning::pruned("ten-batch-8-42", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_preserve_parenthesized_like_0_given_nested_predicates_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT ('a' LIKE 'a')::TEXT LIKE 't%' ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("like-parentheses-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_preserve_parenthesized_like_1_given_nested_predicates_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT 'true' LIKE ('a' LIKE 'a')::TEXT ESCAPE '!' AS output";
    // Act
    let batches = planning::pruned("like-parentheses-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}
