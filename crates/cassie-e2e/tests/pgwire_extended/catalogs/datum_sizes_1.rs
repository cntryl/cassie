//! Native PostgreSQL datum-size witnesses over selected existing scalar inputs.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_measure_input_8_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(CAST(9007199254740993 AS BIGINT)) AS output";
    // Act
    let batches = planning::pruned("datum-input_8_form_0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_measure_input_8_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(CAST(9007199254740993 AS BIGINT)) AS output";
    // Act
    let batches = planning::pruned("datum-input_8_form_1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_measure_input_8_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size(CAST(9007199254740993 AS BIGINT)) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_8_form_2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_measure_input_8_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size(CAST(9007199254740993 AS BIGINT)) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_8_form_3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_measure_input_9_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(CAST(1.5 AS FLOAT)) AS output";
    // Act
    let batches = planning::pruned("datum-input_9_form_0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_measure_input_9_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(CAST(1.5 AS FLOAT)) AS output";
    // Act
    let batches = planning::pruned("datum-input_9_form_1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_measure_input_9_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "WITH q AS (SELECT pg_column_size(CAST(1.5 AS FLOAT)) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_9_form_2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_measure_input_9_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "SELECT output FROM (SELECT pg_catalog.pg_column_size(CAST(1.5 AS FLOAT)) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_9_form_3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_measure_input_10_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(CAST(false AS BOOLEAN)) AS output";
    // Act
    let batches = planning::pruned("datum-input_10_form_0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_measure_input_10_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(CAST(false AS BOOLEAN)) AS output";
    // Act
    let batches = planning::pruned("datum-input_10_form_1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_measure_input_10_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "WITH q AS (SELECT pg_column_size(CAST(false AS BOOLEAN)) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_10_form_2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_measure_input_10_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "SELECT output FROM (SELECT pg_catalog.pg_column_size(CAST(false AS BOOLEAN)) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_10_form_3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_measure_input_11_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(CAST('2024-02-29' AS DATE)) AS output";
    // Act
    let batches = planning::pruned("datum-input_11_form_0", sql);
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
fn should_measure_input_11_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(CAST('2024-02-29' AS DATE)) AS output";
    // Act
    let batches = planning::pruned("datum-input_11_form_1", sql);
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
fn should_measure_input_11_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size(CAST('2024-02-29' AS DATE)) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_11_form_2", sql);
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
fn should_measure_input_11_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size(CAST('2024-02-29' AS DATE)) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_11_form_3", sql);
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
fn should_measure_input_12_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(CAST('12:34:56.000001' AS TIME)) AS output";
    // Act
    let batches = planning::pruned("datum-input_12_form_0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_measure_input_12_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(CAST('12:34:56.000001' AS TIME)) AS output";
    // Act
    let batches = planning::pruned("datum-input_12_form_1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_measure_input_12_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size(CAST('12:34:56.000001' AS TIME)) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_12_form_2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_measure_input_12_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size(CAST('12:34:56.000001' AS TIME)) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_12_form_3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_measure_input_13_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(CAST('2000-01-01 00:00:00.000001' AS TIMESTAMP)) AS output";
    // Act
    let batches = planning::pruned("datum-input_13_form_0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_measure_input_13_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(CAST('2000-01-01 00:00:00.000001' AS TIMESTAMP)) AS output";
    // Act
    let batches = planning::pruned("datum-input_13_form_1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_measure_input_13_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size(CAST('2000-01-01 00:00:00.000001' AS TIMESTAMP)) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_13_form_2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_measure_input_13_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size(CAST('2000-01-01 00:00:00.000001' AS TIMESTAMP)) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_13_form_3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 8]],
        ],
    );
}

#[test]
fn should_measure_input_14_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "SELECT pg_column_size(CAST('00000000-0000-0000-0000-000000000001' AS UUID)) AS output";
    // Act
    let batches = planning::pruned("datum-input_14_form_0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 54]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 16]],
        ],
    );
}

#[test]
fn should_measure_input_14_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(CAST('00000000-0000-0000-0000-000000000001' AS UUID)) AS output";
    // Act
    let batches = planning::pruned("datum-input_14_form_1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 54]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 16]],
        ],
    );
}

#[test]
fn should_measure_input_14_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size(CAST('00000000-0000-0000-0000-000000000001' AS UUID)) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_14_form_2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 54]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 16]],
        ],
    );
}

#[test]
fn should_measure_input_14_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size(CAST('00000000-0000-0000-0000-000000000001' AS UUID)) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_14_form_3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 54]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 16]],
        ],
    );
}

#[test]
fn should_measure_input_15_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(CAST(NULL AS SMALLINT)) AS output";
    // Act
    let batches = planning::pruned("datum-input_15_form_0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_measure_input_15_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(CAST(NULL AS SMALLINT)) AS output";
    // Act
    let batches = planning::pruned("datum-input_15_form_1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_measure_input_15_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "WITH q AS (SELECT pg_column_size(CAST(NULL AS SMALLINT)) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_15_form_2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_measure_input_15_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "SELECT output FROM (SELECT pg_catalog.pg_column_size(CAST(NULL AS SMALLINT)) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_15_form_3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}
