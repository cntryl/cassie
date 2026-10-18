//! Native between predicate folding, typed rows, filtering and aggregate witnesses.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_between_interaction_0_given_range_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT v BETWEEN SYMMETRIC 3 AND 1 AS output FROM (VALUES(0),(1),(2),(3),(4),(NULL::INTEGER)) AS t(v)";
    // Act
    let batches = planning::pruned("between-interaction-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 102],
                &[0, 1, 0, 0, 0, 1, 116],
                &[0, 1, 0, 0, 0, 1, 116],
                &[0, 1, 0, 0, 0, 1, 116],
                &[0, 1, 0, 0, 0, 1, 102],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 1, 0],
                &[0, 1, 0, 0, 0, 1, 1],
                &[0, 1, 0, 0, 0, 1, 1],
                &[0, 1, 0, 0, 0, 1, 1],
                &[0, 1, 0, 0, 0, 1, 0],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_between_interaction_1_given_range_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT v NOT BETWEEN SYMMETRIC 3 AND 1 AS output FROM (VALUES(0),(1),(2),(3),(4),(NULL::INTEGER)) AS t(v)";
    // Act
    let batches = planning::pruned("between-interaction-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 116],
                &[0, 1, 0, 0, 0, 1, 102],
                &[0, 1, 0, 0, 0, 1, 102],
                &[0, 1, 0, 0, 0, 1, 102],
                &[0, 1, 0, 0, 0, 1, 116],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 1, 1],
                &[0, 1, 0, 0, 0, 1, 0],
                &[0, 1, 0, 0, 0, 1, 0],
                &[0, 1, 0, 0, 0, 1, 0],
                &[0, 1, 0, 0, 0, 1, 1],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_between_interaction_2_given_range_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT v AS output FROM (VALUES(0),(1),(2),(3),(4),(NULL::INTEGER)) AS t(v) WHERE v BETWEEN SYMMETRIC 3 AND 1";
    // Act
    let batches = planning::pruned("between-interaction-2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 50],
                &[0, 1, 0, 0, 0, 1, 51],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 2],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 3],
            ],
        ],
    );
}

#[test]
fn should_preserve_between_interaction_3_given_range_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT count(*) AS output FROM (VALUES(0),(1),(2),(3),(4),(NULL::INTEGER)) AS t(v) WHERE v NOT BETWEEN SYMMETRIC 3 AND 1";
    // Act
    let batches = planning::pruned("between-interaction-3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 50]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 2]],
        ],
    );
}

#[test]
fn should_preserve_between_interaction_4_given_range_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT EXISTS(SELECT 1) BETWEEN SYMMETRIC TRUE AND FALSE AS output";
    // Act
    let batches = planning::pruned("between-interaction-4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_preserve_between_interaction_5_given_range_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT (FALSE AND 1/0=0) NOT BETWEEN SYMMETRIC TRUE AND FALSE AS output";
    // Act
    let batches = planning::pruned("between-interaction-5", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_preserve_between_interaction_6_given_range_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT NULL::INTEGER BETWEEN SYMMETRIC 3 AND 1 AS output";
    // Act
    let batches = planning::pruned("between-interaction-6", sql);
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
fn should_preserve_between_interaction_7_given_range_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT 2 BETWEEN SYMMETRIC NULL::INTEGER AND 1 AS output";
    // Act
    let batches = planning::pruned("between-interaction-7", sql);
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
fn should_preserve_between_interaction_8_given_range_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT 2 NOT BETWEEN SYMMETRIC NULL::INTEGER AND 3 AS output";
    // Act
    let batches = planning::pruned("between-interaction-8", sql);
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
fn should_preserve_between_interaction_9_given_range_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT CASE WHEN 2 BETWEEN SYMMETRIC 3 AND 1 THEN 7 ELSE 1/0 END AS output";
    // Act
    let batches = planning::pruned("between-interaction-9", sql);
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
fn should_preserve_between_interaction_10_given_range_input_when_native_rows_are_executed() {
    // Arrange
    let sql =
        "SELECT v BETWEEN SYMMETRIC NULL::INTEGER AND 1 AS output FROM (VALUES(0),(1),(2)) AS t(v)";
    // Act
    let batches = planning::pruned("between-interaction-10", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [
            &[
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_between_interaction_11_given_range_input_when_native_rows_are_executed() {
    // Arrange
    let sql = "SELECT v NOT BETWEEN SYMMETRIC NULL::INTEGER AND 1 AS output FROM (VALUES(0),(1),(2)) AS t(v)";
    // Act
    let batches = planning::pruned("between-interaction-11", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [
            &[
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}
