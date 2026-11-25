//! Native qualified operator and existing-carrier overload witnesses.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_match_bytea_case_0_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT '\\x00ab'::BYTEA || '\\xff'::BYTEA AS v";
    // Act
    let batches = planning::pruned("qualified-bytea-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102]],
            &[&[0, 1, 0, 0, 0, 3, 0, 171, 255]],
        ],
    );
}

#[test]
fn should_match_bytea_case_1_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT '\\x00ab'::BYTEA || '\\xff' AS v";
    // Act
    let batches = planning::pruned("qualified-bytea-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102]],
            &[&[0, 1, 0, 0, 0, 3, 0, 171, 255]],
        ],
    );
}

#[test]
fn should_match_bytea_case_2_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT '\\x00ab' || '\\xff'::BYTEA AS v";
    // Act
    let batches = planning::pruned("qualified-bytea-2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102]],
            &[&[0, 1, 0, 0, 0, 3, 0, 171, 255]],
        ],
    );
}

#[test]
fn should_match_bytea_case_3_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT '\\x00ab'::BYTEA || NULL AS v";
    // Act
    let batches = planning::pruned("qualified-bytea-3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_bytea_case_4_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT NULL || '\\xff'::BYTEA AS v";
    // Act
    let batches = planning::pruned("qualified-bytea-4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_bytea_case_5_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT NULL::BYTEA || NULL::BYTEA AS v";
    // Act
    let batches = planning::pruned("qualified-bytea-5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_bytea_case_6_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT '\\x'::BYTEA || '\\xff'::BYTEA AS v";
    // Act
    let batches = planning::pruned("qualified-bytea-6", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 4, 92, 120, 102, 102]],
            &[&[0, 1, 0, 0, 0, 1, 255]],
        ],
    );
}

#[test]
fn should_match_bytea_case_7_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT '\\x00ab'::BYTEA || 't'::TEXT AS v";
    // Act
    let batches = planning::pruned("qualified-bytea-7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 7, 92, 120, 48, 48, 97, 98, 116]],
            &[&[0, 1, 0, 0, 0, 7, 92, 120, 48, 48, 97, 98, 116]],
        ],
    );
}

#[test]
fn should_match_bytea_case_8_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT 't'::TEXT || '\\x00ab'::BYTEA AS v";
    // Act
    let batches = planning::pruned("qualified-bytea-8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 7, 116, 92, 120, 48, 48, 97, 98]],
            &[&[0, 1, 0, 0, 0, 7, 116, 92, 120, 48, 48, 97, 98]],
        ],
    );
}

#[test]
fn should_match_bytea_case_9_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT '\\x00ab'::BYTEA OPERATOR(pg_catalog.||) '\\xff' AS v";
    // Act
    let batches = planning::pruned("qualified-bytea-9", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102]],
            &[&[0, 1, 0, 0, 0, 3, 0, 171, 255]],
        ],
    );
}

#[test]
fn should_match_bytea_case_10_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT '\\x00ab'::BYTEA OPERATOR(pg_catalog.||) 't'::TEXT AS v";
    // Act
    let batches = planning::pruned("qualified-bytea-10", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 7, 92, 120, 48, 48, 97, 98, 116]],
            &[&[0, 1, 0, 0, 0, 7, 92, 120, 48, 48, 97, 98, 116]],
        ],
    );
}

#[test]
fn should_preserve_text_control_0_given_poisoned_bytea_spelling_when_operand_is_explicit_text() {
    // Arrange
    let sql = "SELECT '\\x00'::BYTEA || '\\xg1'::TEXT AS v";
    // Act
    let batches = planning::pruned("bytea-poison-control-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 92, 120, 103, 49]],
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 92, 120, 103, 49]],
        ],
    );
}

#[test]
fn should_preserve_text_control_1_given_poisoned_bytea_spelling_when_operand_is_explicit_text() {
    // Arrange
    let sql = "SELECT '\\xg1'::TEXT || '\\x00'::BYTEA AS v";
    // Act
    let batches = planning::pruned("bytea-poison-control-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 103, 49, 92, 120, 48, 48]],
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 103, 49, 92, 120, 48, 48]],
        ],
    );
}

#[test]
fn should_preserve_text_origin_0_given_computed_operand_when_bytea_concat_selects_overload() {
    // Arrange
    let sql = "SELECT '\\x00'::BYTEA || COALESCE('\\xg1') AS v";
    // Act
    let batches = planning::pruned("bytea-text-origin-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 92, 120, 103, 49]],
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 92, 120, 103, 49]],
        ],
    );
}

#[test]
fn should_preserve_text_origin_1_given_computed_operand_when_bytea_concat_selects_overload() {
    // Arrange
    let sql = "SELECT '\\x00'::BYTEA || CASE WHEN TRUE THEN '\\xg1' ELSE NULL END AS v";
    // Act
    let batches = planning::pruned("bytea-text-origin-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 92, 120, 103, 49]],
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 92, 120, 103, 49]],
        ],
    );
}

#[test]
fn should_preserve_text_origin_2_given_computed_operand_when_bytea_concat_selects_overload() {
    // Arrange
    let sql = "SELECT '\\x00'::BYTEA || NULLIF('\\xg1','other') AS v";
    // Act
    let batches = planning::pruned("bytea-text-origin-2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 92, 120, 103, 49]],
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 92, 120, 103, 49]],
        ],
    );
}

#[test]
fn should_preserve_text_origin_3_given_computed_operand_when_bytea_concat_selects_overload() {
    // Arrange
    let sql = "SELECT '\\x00'::BYTEA || GREATEST('\\xg1','') AS v";
    // Act
    let batches = planning::pruned("bytea-text-origin-3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 92, 120, 103, 49]],
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 92, 120, 103, 49]],
        ],
    );
}

#[test]
fn should_preserve_text_origin_4_given_computed_operand_when_bytea_concat_selects_overload() {
    // Arrange
    let sql = "SELECT '\\x00'::BYTEA || CAST('\\xg1' AS VARCHAR(8)) AS v";
    // Act
    let batches = planning::pruned("bytea-text-origin-4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 92, 120, 103, 49]],
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 92, 120, 103, 49]],
        ],
    );
}

#[test]
fn should_preserve_text_origin_5_given_computed_operand_when_bytea_concat_selects_overload() {
    // Arrange
    let sql = "WITH q AS (SELECT '\\xg1'::TEXT AS v) SELECT '\\x00'::BYTEA || v AS v FROM q";
    // Act
    let batches = planning::pruned("bytea-text-origin-5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 92, 120, 103, 49]],
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 92, 120, 103, 49]],
        ],
    );
}

#[test]
fn should_preserve_text_origin_6_given_computed_operand_when_bytea_concat_selects_overload() {
    // Arrange
    let sql = "SELECT '\\x00'::BYTEA || v AS v FROM (VALUES ('\\xg1'::TEXT)) AS q(v)";
    // Act
    let batches = planning::pruned("bytea-text-origin-6", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 92, 120, 103, 49]],
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 92, 120, 103, 49]],
        ],
    );
}

#[test]
fn should_preserve_text_origin_7_given_computed_operand_when_bytea_concat_selects_overload() {
    // Arrange
    let sql = "WITH q AS (SELECT COALESCE('\\xg1') AS v) SELECT v || '\\x00'::BYTEA AS v FROM q";
    // Act
    let batches = planning::pruned("bytea-text-origin-7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 103, 49, 92, 120, 48, 48]],
            &[&[0, 1, 0, 0, 0, 8, 92, 120, 103, 49, 92, 120, 48, 48]],
        ],
    );
}
