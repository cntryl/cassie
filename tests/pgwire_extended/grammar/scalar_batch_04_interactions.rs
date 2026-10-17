//! Native unary math interactions with lazy branches and typed rows.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_interaction_0_given_unary_math_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT CASE WHEN FALSE THEN |/(-1)::FLOAT8 ELSE 7::FLOAT8 END AS output";
    // Act
    let batches = planning::pruned("unary-interaction-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 701, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 8, 64, 28, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_interaction_1_given_unary_math_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT @ v AS output FROM (VALUES((-32767)::SMALLINT)) AS t(v)";
    // Act
    let batches = planning::pruned("unary-interaction-1", sql);
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
fn should_preserve_interaction_2_given_unary_math_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT |/v AS output FROM (VALUES(25::BIGINT)) AS t(v)";
    // Act
    let batches = planning::pruned("unary-interaction-2", sql);
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
fn should_preserve_interaction_3_given_unary_math_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT ||/v AS output FROM (VALUES((-8)::INT)) AS t(v)";
    // Act
    let batches = planning::pruned("unary-interaction-3", sql);
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
