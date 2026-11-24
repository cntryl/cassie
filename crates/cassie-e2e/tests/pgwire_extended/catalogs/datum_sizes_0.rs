//! Native PostgreSQL datum-size witnesses over selected existing scalar inputs.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_measure_input_0_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(NULL) AS output";
    // Act
    let batches = planning::pruned("datum-input_0_form_0", sql);
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
fn should_measure_input_0_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(NULL) AS output";
    // Act
    let batches = planning::pruned("datum-input_0_form_1", sql);
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
fn should_measure_input_0_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size(NULL) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_0_form_2", sql);
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
fn should_measure_input_0_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size(NULL) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_0_form_3", sql);
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
fn should_measure_input_1_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size('foo') AS output";
    // Act
    let batches = planning::pruned("datum-input_1_form_0", sql);
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
fn should_measure_input_1_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size('foo') AS output";
    // Act
    let batches = planning::pruned("datum-input_1_form_1", sql);
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
fn should_measure_input_1_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size('foo') AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_1_form_2", sql);
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
fn should_measure_input_1_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size('foo') AS output) q";
    // Act
    let batches = planning::pruned("datum-input_1_form_3", sql);
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
fn should_measure_input_2_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size('é🙂') AS output";
    // Act
    let batches = planning::pruned("datum-input_2_form_0", sql);
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
fn should_measure_input_2_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size('é🙂') AS output";
    // Act
    let batches = planning::pruned("datum-input_2_form_1", sql);
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
fn should_measure_input_2_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size('é🙂') AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_2_form_2", sql);
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
fn should_measure_input_2_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size('é🙂') AS output) q";
    // Act
    let batches = planning::pruned("datum-input_2_form_3", sql);
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
fn should_measure_input_3_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(true) AS output";
    // Act
    let batches = planning::pruned("datum-input_3_form_0", sql);
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
fn should_measure_input_3_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(true) AS output";
    // Act
    let batches = planning::pruned("datum-input_3_form_1", sql);
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
fn should_measure_input_3_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size(true) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_3_form_2", sql);
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
fn should_measure_input_3_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size(true) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_3_form_3", sql);
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
fn should_measure_input_4_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(0) AS output";
    // Act
    let batches = planning::pruned("datum-input_4_form_0", sql);
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
fn should_measure_input_4_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(0) AS output";
    // Act
    let batches = planning::pruned("datum-input_4_form_1", sql);
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
fn should_measure_input_4_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size(0) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_4_form_2", sql);
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
fn should_measure_input_4_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size(0) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_4_form_3", sql);
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
fn should_measure_input_5_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(2147483648) AS output";
    // Act
    let batches = planning::pruned("datum-input_5_form_0", sql);
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
fn should_measure_input_5_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(2147483648) AS output";
    // Act
    let batches = planning::pruned("datum-input_5_form_1", sql);
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
fn should_measure_input_5_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size(2147483648) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_5_form_2", sql);
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
fn should_measure_input_5_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size(2147483648) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_5_form_3", sql);
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
fn should_measure_input_6_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(CAST(-32768 AS SMALLINT)) AS output";
    // Act
    let batches = planning::pruned("datum-input_6_form_0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 50]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 2]],
        ],
    );
}

#[test]
fn should_measure_input_6_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(CAST(-32768 AS SMALLINT)) AS output";
    // Act
    let batches = planning::pruned("datum-input_6_form_1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 50]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 2]],
        ],
    );
}

#[test]
fn should_measure_input_6_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size(CAST(-32768 AS SMALLINT)) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_6_form_2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 50]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 2]],
        ],
    );
}

#[test]
fn should_measure_input_6_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size(CAST(-32768 AS SMALLINT)) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_6_form_3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 50]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 2]],
        ],
    );
}

#[test]
fn should_measure_input_7_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(CAST(2147483647 AS INTEGER)) AS output";
    // Act
    let batches = planning::pruned("datum-input_7_form_0", sql);
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
fn should_measure_input_7_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(CAST(2147483647 AS INTEGER)) AS output";
    // Act
    let batches = planning::pruned("datum-input_7_form_1", sql);
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
fn should_measure_input_7_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size(CAST(2147483647 AS INTEGER)) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_7_form_2", sql);
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
fn should_measure_input_7_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size(CAST(2147483647 AS INTEGER)) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_7_form_3", sql);
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
