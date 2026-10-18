//! Mandatory adversarial nested generic-prefix and cast witnesses.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_prefix_0_given_nested_math_when_native_precedence_is_applied() {
    // Arrange
    let sql = "SELECT @ ~1+2 AS output";
    // Act
    let batches = planning::pruned("unary-nested-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 52]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 4]],
        ],
    );
}

#[test]
fn should_preserve_prefix_1_given_nested_math_when_native_precedence_is_applied() {
    // Arrange
    let sql = "SELECT ~ @ -3+7 AS output";
    // Act
    let batches = planning::pruned("unary-nested-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 53]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 251]],
        ],
    );
}

#[test]
fn should_preserve_prefix_2_given_nested_math_when_native_precedence_is_applied() {
    // Arrange
    let sql = "SELECT |/ |/81 AS output";
    // Act
    let batches = planning::pruned("unary-nested-2", sql);
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
fn should_preserve_prefix_3_given_nested_math_when_native_precedence_is_applied() {
    // Arrange
    let sql = "SELECT ||/ @ -64 AS output";
    // Act
    let batches = planning::pruned("unary-nested-3", sql);
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
fn should_preserve_prefix_4_given_nested_math_when_native_precedence_is_applied() {
    // Arrange
    let sql = "SELECT (@ (-7)::SMALLINT)::BIGINT AS output";
    // Act
    let batches = planning::pruned("unary-nested-4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 7]],
        ],
    );
}

#[test]
fn should_preserve_prefix_5_given_nested_math_when_native_precedence_is_applied() {
    // Arrange
    let sql = "SELECT CASE WHEN FALSE THEN |/(-1)::FLOAT8 ELSE ||/(-8) END AS output";
    // Act
    let batches = planning::pruned("unary-nested-5", sql);
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
fn should_preserve_prefix_6_given_nested_math_when_native_precedence_is_applied() {
    // Arrange
    let sql = "SELECT - @ -7+10 AS output";
    // Act
    let batches = planning::pruned("unary-nested-6", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 51]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 253]],
        ],
    );
}

#[test]
fn should_preserve_prefix_7_given_nested_math_when_native_precedence_is_applied() {
    // Arrange
    let sql = "SELECT - |/9+7 AS output";
    // Act
    let batches = planning::pruned("unary-nested-7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 52]],
            &[&[0, 1, 0, 0, 0, 8, 192, 16, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_prefix_8_given_nested_math_when_native_precedence_is_applied() {
    // Arrange
    let sql = "SELECT @ - ~1+2 AS output";
    // Act
    let batches = planning::pruned("unary-nested-8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 52]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 4]],
        ],
    );
}

#[test]
fn should_preserve_prefix_9_given_nested_math_when_native_precedence_is_applied() {
    // Arrange
    let sql = "SELECT - ~1+2 AS output";
    // Act
    let batches = planning::pruned("unary-nested-9", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 52]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 4]],
        ],
    );
}

#[test]
fn should_preserve_prefix_10_given_nested_math_when_native_precedence_is_applied() {
    // Arrange
    let sql = "SELECT ~ -1+2 AS output";
    // Act
    let batches = planning::pruned("unary-nested-10", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 50]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 254]],
        ],
    );
}

#[test]
fn should_preserve_prefix_11_given_nested_math_when_native_precedence_is_applied() {
    // Arrange
    let sql = "SELECT @ - @ -7+10 AS output";
    // Act
    let batches = planning::pruned("unary-nested-11", sql);
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
fn should_preserve_prefix_12_given_nested_math_when_native_precedence_is_applied() {
    // Arrange
    let sql = "SELECT 2* |/9+7 AS output";
    // Act
    let batches = planning::pruned("unary-nested-12", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 8, 64, 32, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_prefix_13_given_nested_math_when_native_precedence_is_applied() {
    // Arrange
    let sql = "SELECT 2 * - @ -7+10 AS output";
    // Act
    let batches = planning::pruned("unary-nested-13", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 54]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 250]],
        ],
    );
}

#[test]
fn should_preserve_prefix_14_given_nested_math_when_native_precedence_is_applied() {
    // Arrange
    let sql = "SELECT 2 + 3* |/9+7 AS output";
    // Act
    let batches = planning::pruned("unary-nested-14", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 52]],
            &[&[0, 1, 0, 0, 0, 8, 64, 44, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_prefix_15_given_nested_math_when_native_precedence_is_applied() {
    // Arrange
    let sql = "SELECT 2^ |/9+7 AS output";
    // Act
    let batches = planning::pruned("unary-nested-15", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 54]],
            &[&[0, 1, 0, 0, 0, 8, 64, 48, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_prefix_16_given_nested_math_when_native_precedence_is_applied() {
    // Arrange
    let sql = "SELECT 1 | ~1+2 AS output";
    // Act
    let batches = planning::pruned("unary-nested-16", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 45, 51]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 253]],
        ],
    );
}

#[test]
fn should_preserve_prefix_17_given_nested_math_when_native_precedence_is_applied() {
    // Arrange
    let sql = "SELECT 3 + @ -7+10 AS output";
    // Act
    let batches = planning::pruned("unary-nested-17", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 54]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 6]],
        ],
    );
}
