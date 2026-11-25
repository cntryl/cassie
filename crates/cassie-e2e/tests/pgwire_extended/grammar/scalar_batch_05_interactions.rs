//! Native truth predicate folding, typed rows, filtering and aggregate witnesses.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_truth_interaction_0_given_boolean_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT TRUE IS TRUE IS FALSE AS output";
    // Act
    let batches = planning::pruned("truth-interaction-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_preserve_truth_interaction_1_given_boolean_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT NULL IS UNKNOWN IS TRUE AS output";
    // Act
    let batches = planning::pruned("truth-interaction-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_preserve_truth_interaction_2_given_boolean_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT (FALSE AND 1/0=1) IS FALSE AS output";
    // Act
    let batches = planning::pruned("truth-interaction-2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_preserve_truth_interaction_3_given_boolean_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT (NULL::BOOLEAN AND FALSE) IS FALSE AS output";
    // Act
    let batches = planning::pruned("truth-interaction-3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_preserve_truth_interaction_4_given_boolean_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT (NULL::BOOLEAN OR FALSE) IS UNKNOWN AS output";
    // Act
    let batches = planning::pruned("truth-interaction-4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_preserve_truth_interaction_5_given_boolean_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT CASE WHEN (FALSE AND 1/0=0) IS FALSE THEN TRUE ELSE FALSE END AS output";
    // Act
    let batches = planning::pruned("truth-interaction-5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_preserve_truth_interaction_6_given_boolean_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT v IS UNKNOWN AS output FROM (VALUES(TRUE),(FALSE),(NULL::BOOLEAN)) AS t(v)";
    // Act
    let batches = planning::pruned("truth-interaction-6", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 102],
                &[0, 1, 0, 0, 0, 1, 102],
                &[0, 1, 0, 0, 0, 1, 116],
            ],
            &[
                &[0, 1, 0, 0, 0, 1, 0],
                &[0, 1, 0, 0, 0, 1, 0],
                &[0, 1, 0, 0, 0, 1, 1],
            ],
        ],
    );
}

#[test]
fn should_preserve_truth_interaction_7_given_boolean_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT v IS NOT FALSE AS output FROM (VALUES(TRUE),(FALSE),(NULL::BOOLEAN)) AS t(v)";
    // Act
    let batches = planning::pruned("truth-interaction-7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 116],
                &[0, 1, 0, 0, 0, 1, 102],
                &[0, 1, 0, 0, 0, 1, 116],
            ],
            &[
                &[0, 1, 0, 0, 0, 1, 1],
                &[0, 1, 0, 0, 0, 1, 0],
                &[0, 1, 0, 0, 0, 1, 1],
            ],
        ],
    );
}

#[test]
fn should_preserve_truth_interaction_8_given_boolean_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT v IS TRUE AS output FROM (VALUES(TRUE),(FALSE),(NULL::BOOLEAN)) AS t(v) WHERE v IS NOT FALSE";
    // Act
    let batches = planning::pruned("truth-interaction-8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 116], &[0, 1, 0, 0, 0, 1, 102]],
            &[&[0, 1, 0, 0, 0, 1, 1], &[0, 1, 0, 0, 0, 1, 0]],
        ],
    );
}

#[test]
fn should_preserve_truth_interaction_9_given_boolean_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT count(*) AS output FROM (VALUES(TRUE),(FALSE),(NULL::BOOLEAN)) AS t(v) WHERE v IS UNKNOWN";
    // Act
    let batches = planning::pruned("truth-interaction-9", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_preserve_truth_interaction_10_given_boolean_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT coalesce(NULL::BOOLEAN,FALSE) IS UNKNOWN AS output";
    // Act
    let batches = planning::pruned("truth-interaction-10", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_preserve_truth_interaction_11_given_boolean_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT EXISTS(SELECT 1) IS TRUE AS output";
    // Act
    let batches = planning::pruned("truth-interaction-11", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}
