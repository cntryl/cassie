//! Native PostgreSQL datum-size witnesses over selected existing scalar inputs.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_measure_input_16_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(CAST(NULL AS INTEGER)) AS output";
    // Act
    let batches = planning::pruned("datum-input_16_form_0", sql);
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
fn should_measure_input_16_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(CAST(NULL AS INTEGER)) AS output";
    // Act
    let batches = planning::pruned("datum-input_16_form_1", sql);
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
fn should_measure_input_16_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "WITH q AS (SELECT pg_column_size(CAST(NULL AS INTEGER)) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_16_form_2", sql);
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
fn should_measure_input_16_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "SELECT output FROM (SELECT pg_catalog.pg_column_size(CAST(NULL AS INTEGER)) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_16_form_3", sql);
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
fn should_measure_input_17_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(CAST(NULL AS BIGINT)) AS output";
    // Act
    let batches = planning::pruned("datum-input_17_form_0", sql);
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
fn should_measure_input_17_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(CAST(NULL AS BIGINT)) AS output";
    // Act
    let batches = planning::pruned("datum-input_17_form_1", sql);
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
fn should_measure_input_17_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "WITH q AS (SELECT pg_column_size(CAST(NULL AS BIGINT)) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_17_form_2", sql);
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
fn should_measure_input_17_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "SELECT output FROM (SELECT pg_catalog.pg_column_size(CAST(NULL AS BIGINT)) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_17_form_3", sql);
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
fn should_measure_input_18_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(CAST(NULL AS FLOAT)) AS output";
    // Act
    let batches = planning::pruned("datum-input_18_form_0", sql);
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
fn should_measure_input_18_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(CAST(NULL AS FLOAT)) AS output";
    // Act
    let batches = planning::pruned("datum-input_18_form_1", sql);
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
fn should_measure_input_18_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "WITH q AS (SELECT pg_column_size(CAST(NULL AS FLOAT)) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_18_form_2", sql);
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
fn should_measure_input_18_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "SELECT output FROM (SELECT pg_catalog.pg_column_size(CAST(NULL AS FLOAT)) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_18_form_3", sql);
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
fn should_measure_input_19_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(CAST(NULL AS BOOLEAN)) AS output";
    // Act
    let batches = planning::pruned("datum-input_19_form_0", sql);
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
fn should_measure_input_19_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(CAST(NULL AS BOOLEAN)) AS output";
    // Act
    let batches = planning::pruned("datum-input_19_form_1", sql);
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
fn should_measure_input_19_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "WITH q AS (SELECT pg_column_size(CAST(NULL AS BOOLEAN)) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_19_form_2", sql);
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
fn should_measure_input_19_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "SELECT output FROM (SELECT pg_catalog.pg_column_size(CAST(NULL AS BOOLEAN)) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_19_form_3", sql);
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
fn should_measure_input_20_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(CAST(NULL AS DATE)) AS output";
    // Act
    let batches = planning::pruned("datum-input_20_form_0", sql);
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
fn should_measure_input_20_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(CAST(NULL AS DATE)) AS output";
    // Act
    let batches = planning::pruned("datum-input_20_form_1", sql);
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
fn should_measure_input_20_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "WITH q AS (SELECT pg_column_size(CAST(NULL AS DATE)) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_20_form_2", sql);
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
fn should_measure_input_20_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "SELECT output FROM (SELECT pg_catalog.pg_column_size(CAST(NULL AS DATE)) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_20_form_3", sql);
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
fn should_measure_input_21_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(CAST(NULL AS TIME)) AS output";
    // Act
    let batches = planning::pruned("datum-input_21_form_0", sql);
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
fn should_measure_input_21_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(CAST(NULL AS TIME)) AS output";
    // Act
    let batches = planning::pruned("datum-input_21_form_1", sql);
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
fn should_measure_input_21_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "WITH q AS (SELECT pg_column_size(CAST(NULL AS TIME)) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_21_form_2", sql);
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
fn should_measure_input_21_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "SELECT output FROM (SELECT pg_catalog.pg_column_size(CAST(NULL AS TIME)) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_21_form_3", sql);
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
fn should_measure_input_22_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(CAST(NULL AS TIMESTAMP)) AS output";
    // Act
    let batches = planning::pruned("datum-input_22_form_0", sql);
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
fn should_measure_input_22_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(CAST(NULL AS TIMESTAMP)) AS output";
    // Act
    let batches = planning::pruned("datum-input_22_form_1", sql);
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
fn should_measure_input_22_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "WITH q AS (SELECT pg_column_size(CAST(NULL AS TIMESTAMP)) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_22_form_2", sql);
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
fn should_measure_input_22_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size(CAST(NULL AS TIMESTAMP)) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_22_form_3", sql);
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
fn should_measure_input_23_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(CAST(NULL AS UUID)) AS output";
    // Act
    let batches = planning::pruned("datum-input_23_form_0", sql);
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
fn should_measure_input_23_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(CAST(NULL AS UUID)) AS output";
    // Act
    let batches = planning::pruned("datum-input_23_form_1", sql);
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
fn should_measure_input_23_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "WITH q AS (SELECT pg_column_size(CAST(NULL AS UUID)) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_23_form_2", sql);
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
fn should_measure_input_23_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "SELECT output FROM (SELECT pg_catalog.pg_column_size(CAST(NULL AS UUID)) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_23_form_3", sql);
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
