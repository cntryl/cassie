//! Native nested-cast provenance and width witnesses.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_measure_nested_input_0_form_0_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size('é🙂x'::VARCHAR(2)::TEXT) AS output";
    // Act
    let batches = planning::pruned("nested-input_0_form_0", sql);
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
fn should_measure_nested_input_0_form_1_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size('é🙂x'::VARCHAR(2)::TEXT) AS output";
    // Act
    let batches = planning::pruned("nested-input_0_form_1", sql);
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
fn should_measure_nested_input_0_form_2_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size('é🙂x'::VARCHAR(2)::TEXT) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("nested-input_0_form_2", sql);
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
fn should_measure_nested_input_0_form_3_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size('é🙂x'::VARCHAR(2)::TEXT) AS output) q";
    // Act
    let batches = planning::pruned("nested-input_0_form_3", sql);
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
fn should_measure_nested_input_1_form_0_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size('abcdef'::TEXT::VARCHAR(3)) AS output";
    // Act
    let batches = planning::pruned("nested-input_1_form_0", sql);
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
fn should_measure_nested_input_1_form_1_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size('abcdef'::TEXT::VARCHAR(3)) AS output";
    // Act
    let batches = planning::pruned("nested-input_1_form_1", sql);
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
fn should_measure_nested_input_1_form_2_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size('abcdef'::TEXT::VARCHAR(3)) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("nested-input_1_form_2", sql);
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
fn should_measure_nested_input_1_form_3_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size('abcdef'::TEXT::VARCHAR(3)) AS output) q";
    // Act
    let batches = planning::pruned("nested-input_1_form_3", sql);
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
fn should_measure_nested_input_2_form_0_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size('\\x00abff'::BYTEA::TEXT) AS output";
    // Act
    let batches = planning::pruned("nested-input_2_form_0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 50]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 12]],
        ],
    );
}

#[test]
fn should_measure_nested_input_2_form_1_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size('\\x00abff'::BYTEA::TEXT) AS output";
    // Act
    let batches = planning::pruned("nested-input_2_form_1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 50]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 12]],
        ],
    );
}

#[test]
fn should_measure_nested_input_2_form_2_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size('\\x00abff'::BYTEA::TEXT) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("nested-input_2_form_2", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 50]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 12]],
        ],
    );
}

#[test]
fn should_measure_nested_input_2_form_3_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size('\\x00abff'::BYTEA::TEXT) AS output) q";
    // Act
    let batches = planning::pruned("nested-input_2_form_3", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 2, 49, 50]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 12]],
        ],
    );
}

#[test]
fn should_measure_nested_input_3_form_0_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size('\\x00abff'::TEXT::BYTEA) AS output";
    // Act
    let batches = planning::pruned("nested-input_3_form_0", sql);
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
fn should_measure_nested_input_3_form_1_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size('\\x00abff'::TEXT::BYTEA) AS output";
    // Act
    let batches = planning::pruned("nested-input_3_form_1", sql);
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
fn should_measure_nested_input_3_form_2_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size('\\x00abff'::TEXT::BYTEA) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("nested-input_3_form_2", sql);
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
fn should_measure_nested_input_3_form_3_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size('\\x00abff'::TEXT::BYTEA) AS output) q";
    // Act
    let batches = planning::pruned("nested-input_3_form_3", sql);
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
fn should_measure_nested_input_4_form_0_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(NULL::BYTEA::TEXT) AS output";
    // Act
    let batches = planning::pruned("nested-input_4_form_0", sql);
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
fn should_measure_nested_input_4_form_1_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(NULL::BYTEA::TEXT) AS output";
    // Act
    let batches = planning::pruned("nested-input_4_form_1", sql);
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
fn should_measure_nested_input_4_form_2_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size(NULL::BYTEA::TEXT) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("nested-input_4_form_2", sql);
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
fn should_measure_nested_input_4_form_3_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql =
        "SELECT output FROM (SELECT pg_catalog.pg_column_size(NULL::BYTEA::TEXT) AS output) q";
    // Act
    let batches = planning::pruned("nested-input_4_form_3", sql);
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
fn should_measure_nested_input_5_form_0_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size('foo'::VARCHAR(8)::VARCHAR(2)::TEXT) AS output";
    // Act
    let batches = planning::pruned("nested-input_5_form_0", sql);
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

#[test]
fn should_measure_nested_input_5_form_1_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size('foo'::VARCHAR(8)::VARCHAR(2)::TEXT) AS output";
    // Act
    let batches = planning::pruned("nested-input_5_form_1", sql);
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

#[test]
fn should_measure_nested_input_5_form_2_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size('foo'::VARCHAR(8)::VARCHAR(2)::TEXT) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("nested-input_5_form_2", sql);
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

#[test]
fn should_measure_nested_input_5_form_3_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size('foo'::VARCHAR(8)::VARCHAR(2)::TEXT) AS output) q";
    // Act
    let batches = planning::pruned("nested-input_5_form_3", sql);
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

#[test]
fn should_measure_nested_input_6_form_0_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size((1+1)::SMALLINT) AS output";
    // Act
    let batches = planning::pruned("nested-input_6_form_0", sql);
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
fn should_measure_nested_input_6_form_1_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size((1+1)::SMALLINT) AS output";
    // Act
    let batches = planning::pruned("nested-input_6_form_1", sql);
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
fn should_measure_nested_input_6_form_2_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "WITH q AS (SELECT pg_column_size((1+1)::SMALLINT) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("nested-input_6_form_2", sql);
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
fn should_measure_nested_input_6_form_3_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT output FROM (SELECT pg_catalog.pg_column_size((1+1)::SMALLINT) AS output) q";
    // Act
    let batches = planning::pruned("nested-input_6_form_3", sql);
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
fn should_measure_nested_input_7_form_0_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_column_size(('foo'::TEXT)::TEXT) AS output";
    // Act
    let batches = planning::pruned("nested-input_7_form_0", sql);
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
fn should_measure_nested_input_7_form_1_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(('foo'::TEXT)::TEXT) AS output";
    // Act
    let batches = planning::pruned("nested-input_7_form_1", sql);
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
fn should_measure_nested_input_7_form_2_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql =
        "WITH q AS (SELECT pg_column_size(('foo'::TEXT)::TEXT) AS output) SELECT output FROM q";
    // Act
    let batches = planning::pruned("nested-input_7_form_2", sql);
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
fn should_measure_nested_input_7_form_3_given_cast_chains_when_column_size_is_executed() {
    // Arrange
    let sql =
        "SELECT output FROM (SELECT pg_catalog.pg_column_size(('foo'::TEXT)::TEXT) AS output) q";
    // Act
    let batches = planning::pruned("nested-input_7_form_3", sql);
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
