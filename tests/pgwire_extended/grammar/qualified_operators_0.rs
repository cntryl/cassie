//! Pinned PostgreSQL18.6 qualified builtin operator witnesses.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_match_case_0_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT 1 OPERATOR(pg_catalog.+) 2 AS v";
    // Act
    let batches = planning::pruned("qualified-op-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 51]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 3]],
        ],
    );
}

#[test]
fn should_match_case_1_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT OPERATOR(pg_catalog.-) 2 AS v";
    // Act
    let batches = planning::pruned("qualified-op-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 50]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 254]],
        ],
    );
}

#[test]
fn should_match_case_2_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT 2 OPERATOR(pg_catalog.*) 3 + 4 AS v";
    // Act
    let batches = planning::pruned("qualified-op-2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 52]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 14]],
        ],
    );
}

#[test]
fn should_match_case_3_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT 2 + 3 OPERATOR(pg_catalog.*) 4 AS v";
    // Act
    let batches = planning::pruned("qualified-op-3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 50, 48]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 20]],
        ],
    );
}

#[test]
fn should_match_case_4_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT OPERATOR(pg_catalog.-) 2 + 3 AS v";
    // Act
    let batches = planning::pruned("qualified-op-4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 53]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 251]],
        ],
    );
}

#[test]
fn should_match_case_5_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT 2 OPERATOR(pg_catalog.^) 3 + 1 AS v";
    // Act
    let batches = planning::pruned("qualified-op-5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 54]],
            &[&[0, 1, 0, 0, 0, 8, 64, 48, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_6_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT 2 OPERATOR(pg_catalog.+) 3 OPERATOR(pg_catalog.*) 4 AS v";
    // Act
    let batches = planning::pruned("qualified-op-6", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 50, 48]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 20]],
        ],
    );
}

#[test]
fn should_match_case_7_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT 1 OPERATOR(pg_catalog.=) 1 AS v";
    // Act
    let batches = planning::pruned("qualified-op-7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_8_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT 1 OPERATOR(pg_catalog.<) 2 AS v";
    // Act
    let batches = planning::pruned("qualified-op-8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_9_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT NULL::INT OPERATOR(pg_catalog.+) 2 AS v";
    // Act
    let batches = planning::pruned("qualified-op-9", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 20, 8, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_match_case_10_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT OPERATOR(pg_catalog.~) 3 AS v";
    // Act
    let batches = planning::pruned("qualified-op-10", sql);
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
fn should_match_case_11_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT OPERATOR(pg_catalog.@) -7 + 10 AS v";
    // Act
    let batches = planning::pruned("qualified-op-11", sql);
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
fn should_match_case_12_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT 1 OPERATOR(\"pg_catalog\".+) 2 AS v";
    // Act
    let batches = planning::pruned("qualified-op-12", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 51]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 3]],
        ],
    );
}

#[test]
fn should_match_case_13_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT 1 OPERATOR(PG_CATALOG.+) 2 AS v";
    // Act
    let batches = planning::pruned("qualified-op-13", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 51]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 3]],
        ],
    );
}

#[test]
fn should_match_case_14_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT 'a' OPERATOR(pg_catalog.||) 'b' AS v";
    // Act
    let batches = planning::pruned("qualified-op-14", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 97, 98]],
            &[&[0, 1, 0, 0, 0, 2, 97, 98]],
        ],
    );
}

#[test]
fn should_match_case_15_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT 6 OPERATOR(pg_catalog.&) 3 AS v";
    // Act
    let batches = planning::pruned("qualified-op-15", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 50]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 2]],
        ],
    );
}

#[test]
fn should_match_case_16_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT 5 OPERATOR(pg_catalog.%) 2 AS v";
    // Act
    let batches = planning::pruned("qualified-op-16", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_match_case_17_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT 4 OPERATOR(pg_catalog.<<) 2 AS v";
    // Act
    let batches = planning::pruned("qualified-op-17", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 54]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 16]],
        ],
    );
}

#[test]
fn should_match_case_18_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT TRUE AND 1 OPERATOR(pg_catalog.=) 1 AS v";
    // Act
    let batches = planning::pruned("qualified-op-18", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_19_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT 1 OPERATOR(pg_catalog.=) 1 IS TRUE AS v";
    // Act
    let batches = planning::pruned("qualified-op-19", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}
