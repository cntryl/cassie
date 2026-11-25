//! Native qualified operator and existing-carrier overload witnesses.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_match_mixed_case_0_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT 1 OPERATOR(pg_catalog.+) 2 * 3 AS v";
    // Act
    let batches = planning::pruned("qualified-mixed-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_match_mixed_case_1_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT 1 + 2 OPERATOR(pg_catalog.*) 3 AS v";
    // Act
    let batches = planning::pruned("qualified-mixed-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 57]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 9]],
        ],
    );
}

#[test]
fn should_match_mixed_case_2_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT 1 OPERATOR(pg_catalog.<<) 2 | 1 AS v";
    // Act
    let batches = planning::pruned("qualified-mixed-2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 53]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 5]],
        ],
    );
}

#[test]
fn should_match_mixed_case_3_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT 1 | 2 OPERATOR(pg_catalog.<<) 1 AS v";
    // Act
    let batches = planning::pruned("qualified-mixed-3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 54]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 6]],
        ],
    );
}

#[test]
fn should_match_mixed_case_4_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT 1 OPERATOR(pg_catalog.<<) 2 + 1 AS v";
    // Act
    let batches = planning::pruned("qualified-mixed-4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_match_mixed_case_5_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT 1+1 OPERATOR(pg_catalog.<<) 2 AS v";
    // Act
    let batches = planning::pruned("qualified-mixed-5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_match_mixed_case_6_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT 1 OPERATOR(pg_catalog.=) 1 AND TRUE AS v";
    // Act
    let batches = planning::pruned("qualified-mixed-6", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_mixed_case_7_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT 1 OPERATOR(pg_catalog.<) 2 = TRUE AS v";
    // Act
    let batches = planning::pruned("qualified-mixed-7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_mixed_case_8_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT 1 < 2 OPERATOR(pg_catalog.+) 3 AS v";
    // Act
    let batches = planning::pruned("qualified-mixed-8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_mixed_case_9_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT 2 OPERATOR(pg_catalog.-) 1 OPERATOR(pg_catalog.*) 3 AS v";
    // Act
    let batches = planning::pruned("qualified-mixed-9", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 51]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 3]],
        ],
    );
}

#[test]
fn should_match_mixed_case_10_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT 2 OPERATOR(pg_catalog.-) 1 * 3 AS v";
    // Act
    let batches = planning::pruned("qualified-mixed-10", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 49]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_mixed_case_11_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT OPERATOR(pg_catalog.-) 2 OPERATOR(pg_catalog.+) 3 AS v";
    // Act
    let batches = planning::pruned("qualified-mixed-11", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_match_mixed_case_12_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT - OPERATOR(pg_catalog.-) 2 OPERATOR(pg_catalog.+) 3 AS v";
    // Act
    let batches = planning::pruned("qualified-mixed-12", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 53]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 5]],
        ],
    );
}

#[test]
fn should_match_mixed_case_13_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT OPERATOR(pg_catalog.-)-2+3 AS v";
    // Act
    let batches = planning::pruned("qualified-mixed-13", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 49]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_mixed_case_14_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT OPERATOR(pg_catalog.~) OPERATOR(pg_catalog.~)1 AS v";
    // Act
    let batches = planning::pruned("qualified-mixed-14", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_match_mixed_case_15_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT 1 OPERATOR(pg_catalog.+) OPERATOR(pg_catalog.-)2+3 AS v";
    // Act
    let batches = planning::pruned("qualified-mixed-15", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 52]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 252]],
        ],
    );
}

#[test]
fn should_match_mixed_case_16_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT 1 OPERATOR(pg_catalog.+) - OPERATOR(pg_catalog.-)2+3 AS v";
    // Act
    let batches = planning::pruned("qualified-mixed-16", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 54]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 6]],
        ],
    );
}

#[test]
fn should_match_mixed_case_17_given_builtin_operator_when_native_value_is_executed() {
    // Arrange
    let sql = "SELECT 1 + OPERATOR(pg_catalog.-)2+3 AS v";
    // Act
    let batches = planning::pruned("qualified-mixed-17", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 52]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 252]],
        ],
    );
}
