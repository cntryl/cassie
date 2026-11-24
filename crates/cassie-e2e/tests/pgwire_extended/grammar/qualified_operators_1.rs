//! Pinned PostgreSQL18.6 qualified builtin operator witnesses.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_match_case_20_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT - OPERATOR(pg_catalog.-) 2 + 3 AS v";
    // Act
    let batches = planning::pruned("qualified-op-20", sql);
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
fn should_match_case_21_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT OPERATOR(pg_catalog.-) 2 ^ 2 AS v";
    // Act
    let batches = planning::pruned("qualified-op-21", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 52]],
            &[&[0, 1, 0, 0, 0, 8, 192, 16, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_22_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT -2 ^ 2 AS v";
    // Act
    let batches = planning::pruned("qualified-op-22", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 52]],
            &[&[0, 1, 0, 0, 0, 8, 64, 16, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_23_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT 1 OPERATOR(pg_catalog.+)-2 AS v";
    // Act
    let batches = planning::pruned("qualified-op-23", sql);
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
fn should_match_case_24_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT OPERATOR(pg_catalog.~)~1 AS v";
    // Act
    let batches = planning::pruned("qualified-op-24", sql);
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
fn should_match_case_25_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT OPERATOR(pg_catalog.|/) 9::FLOAT AS v";
    // Act
    let batches = planning::pruned("qualified-op-25", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 51]],
            &[&[0, 1, 0, 0, 0, 8, 64, 8, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_26_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT OPERATOR(pg_catalog.||/) 8::FLOAT AS v";
    // Act
    let batches = planning::pruned("qualified-op-26", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 50]],
            &[&[0, 1, 0, 0, 0, 8, 64, 0, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_match_case_27_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT 'value=' OPERATOR(pg_catalog.||) 42 AS v";
    // Act
    let batches = planning::pruned("qualified-op-27", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 8, 118, 97, 108, 117, 101, 61, 52, 50]],
            &[&[0, 1, 0, 0, 0, 8, 118, 97, 108, 117, 101, 61, 52, 50]],
        ],
    );
}

#[test]
fn should_match_case_28_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT '\\x00ab'::BYTEA OPERATOR(pg_catalog.||) '\\xff'::BYTEA AS v";
    // Act
    let batches = planning::pruned("qualified-op-28", sql);
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
fn should_match_case_29_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT TRUE OPERATOR(pg_catalog.=) FALSE AS v";
    // Act
    let batches = planning::pruned("qualified-op-29", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_match_case_30_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT 1 OPERATOR(pg_catalog.=) 1 OPERATOR(pg_catalog.=) TRUE AS v";
    // Act
    let batches = planning::pruned("qualified-op-30", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_match_case_31_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT 1 OPERATOR(/* before */ pg_catalog /* schema */ . + /* op */) 2 AS v";
    // Act
    let batches = planning::pruned("qualified-op-31", sql);
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
fn should_match_case_32_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT 'operator(pg_catalog.+)' AS v";
    // Act
    let batches = planning::pruned("qualified-op-32", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 25, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 22, 111, 112, 101, 114, 97, 116, 111, 114, 40, 112, 103, 95, 99, 97,
                116, 97, 108, 111, 103, 46, 43, 41,
            ]],
            &[&[
                0, 1, 0, 0, 0, 22, 111, 112, 101, 114, 97, 116, 111, 114, 40, 112, 103, 95, 99, 97,
                116, 97, 108, 111, 103, 46, 43, 41,
            ]],
        ],
    );
}

#[test]
fn should_match_case_33_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT GREATEST(2 OPERATOR(pg_catalog.*) 3 + 4,10) AS v";
    // Act
    let batches = planning::pruned("qualified-op-33", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 52]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 14]],
        ],
    );
}

#[test]
fn should_match_case_34_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT CASE WHEN 1 OPERATOR(pg_catalog.=) 1 THEN 7 ELSE 0 END AS v";
    // Act
    let batches = planning::pruned("qualified-op-34", sql);
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
fn should_match_case_35_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "WITH q(n) AS (VALUES (2)) SELECT n OPERATOR(pg_catalog.*) 3 + 4 AS v FROM q";
    // Act
    let batches = planning::pruned("qualified-op-35", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 52]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 14]],
        ],
    );
}

#[test]
fn should_match_case_36_given_qualified_builtin_when_native_operator_is_executed() {
    // Arrange
    let sql = "SELECT n OPERATOR(pg_catalog.*) 3 + 4 AS v FROM (VALUES (2)) AS q(n)";
    // Act
    let batches = planning::pruned("qualified-op-36", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 52]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 14]],
        ],
    );
}
