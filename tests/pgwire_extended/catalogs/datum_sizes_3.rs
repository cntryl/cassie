//! Native PostgreSQL datum-size witnesses over selected existing scalar inputs.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_measure_input_24_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(''::TEXT) AS output";
    // Act
    let batches = planning::pruned("datum-input_24_form_0", sql);
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
fn should_measure_input_24_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(''::TEXT) AS output";
    // Act
    let batches = planning::pruned("datum-input_24_form_1", sql);
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
fn should_measure_input_24_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size(''::TEXT) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_24_form_2", sql);
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
fn should_measure_input_24_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size(''::TEXT) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_24_form_3", sql);
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
fn should_measure_input_25_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size('foo'::TEXT) AS output";
    // Act
    let batches = planning::pruned("datum-input_25_form_0", sql);
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
fn should_measure_input_25_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size('foo'::TEXT) AS output";
    // Act
    let batches = planning::pruned("datum-input_25_form_1", sql);
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
fn should_measure_input_25_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size('foo'::TEXT) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_25_form_2", sql);
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
fn should_measure_input_25_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size('foo'::TEXT) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_25_form_3", sql);
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
fn should_measure_input_26_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size('é🙂'::TEXT) AS output";
    // Act
    let batches = planning::pruned("datum-input_26_form_0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 10]],
        ],
    );
}

#[test]
fn should_measure_input_26_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size('é🙂'::TEXT) AS output";
    // Act
    let batches = planning::pruned("datum-input_26_form_1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 10]],
        ],
    );
}

#[test]
fn should_measure_input_26_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size('é🙂'::TEXT) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_26_form_2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 10]],
        ],
    );
}

#[test]
fn should_measure_input_26_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size('é🙂'::TEXT) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_26_form_3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 10]],
        ],
    );
}

#[test]
fn should_measure_input_27_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size('abcdef'::VARCHAR(3)) AS output";
    // Act
    let batches = planning::pruned("datum-input_27_form_0", sql);
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
fn should_measure_input_27_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size('abcdef'::VARCHAR(3)) AS output";
    // Act
    let batches = planning::pruned("datum-input_27_form_1", sql);
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
fn should_measure_input_27_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "WITH q AS (SELECT pg_column_size('abcdef'::VARCHAR(3)) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_27_form_2", sql);
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
fn should_measure_input_27_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "SELECT output FROM (SELECT pg_catalog.pg_column_size('abcdef'::VARCHAR(3)) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_27_form_3", sql);
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
fn should_measure_input_28_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size('\\x'::BYTEA) AS output";
    // Act
    let batches = planning::pruned("datum-input_28_form_0", sql);
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
fn should_measure_input_28_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size('\\x'::BYTEA) AS output";
    // Act
    let batches = planning::pruned("datum-input_28_form_1", sql);
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
fn should_measure_input_28_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size('\\x'::BYTEA) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_28_form_2", sql);
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
fn should_measure_input_28_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size('\\x'::BYTEA) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_28_form_3", sql);
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
fn should_measure_input_29_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size('\\x00abff'::BYTEA) AS output";
    // Act
    let batches = planning::pruned("datum-input_29_form_0", sql);
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
fn should_measure_input_29_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size('\\x00abff'::BYTEA) AS output";
    // Act
    let batches = planning::pruned("datum-input_29_form_1", sql);
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
fn should_measure_input_29_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "WITH q AS (SELECT pg_column_size('\\x00abff'::BYTEA) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_29_form_2", sql);
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
fn should_measure_input_29_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql =
        "SELECT output FROM (SELECT pg_catalog.pg_column_size('\\x00abff'::BYTEA) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_29_form_3", sql);
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
fn should_measure_input_30_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size('ééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééé'::TEXT) AS output";
    // Act
    let batches = planning::pruned("datum-input_30_form_0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 50, 53, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 1, 2]],
        ],
    );
}

#[test]
fn should_measure_input_30_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size('ééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééé'::TEXT) AS output";
    // Act
    let batches = planning::pruned("datum-input_30_form_1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 50, 53, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 1, 2]],
        ],
    );
}

#[test]
fn should_measure_input_30_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size('ééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééé'::TEXT) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_30_form_2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 50, 53, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 1, 2]],
        ],
    );
}

#[test]
fn should_measure_input_30_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size('ééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééé'::TEXT) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_30_form_3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 50, 53, 56]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 1, 2]],
        ],
    );
}

#[test]
fn should_measure_input_31_form_0_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(NULL::TEXT) AS output";
    // Act
    let batches = planning::pruned("datum-input_31_form_0", sql);
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
fn should_measure_input_31_form_1_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(NULL::TEXT) AS output";
    // Act
    let batches = planning::pruned("datum-input_31_form_1", sql);
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
fn should_measure_input_31_form_2_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size(NULL::TEXT) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("datum-input_31_form_2", sql);
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
fn should_measure_input_31_form_3_given_declared_inputs_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size(NULL::TEXT) AS output) q";
    // Act
    let batches = planning::pruned("datum-input_31_form_3", sql);
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
