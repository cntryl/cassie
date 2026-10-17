//! Native declared and inferred cast-input identities across all wire formats.
use crate::support_values_casts;

#[test]
fn should_evaluate_inferred_integer_alias_given_identifier_metadata_when_input_and_output_formats_are_crossed(
) {
    // Arrange
    let sql =
        "SELECT \"Parameter Alias\".v AS output FROM (SELECT $1::INT4 AS v) AS \"Parameter Alias\"";
    // Act
    let batches = support_values_casts::parameters(
        "identifier-metadata-inferred_integer_alias",
        sql,
        0,
        &[55],
        &[0, 0, 0, 7],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 23);
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
fn should_evaluate_declared_char_alias_given_identifier_metadata_when_input_and_output_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT \"Parameter Alias\".v AS output FROM (SELECT $1::CHAR(3) AS v) AS \"Parameter Alias\"";
    // Act
    let batches = support_values_casts::parameters(
        "identifier-metadata-declared_char_alias",
        sql,
        1042,
        &[97],
        &[97],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 1042);
    support_values_casts::assert_result(
        &batches,
        ("output", 1042, -1, 7),
        [
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32]],
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32]],
        ],
    );
}

#[test]
fn should_evaluate_inferred_cte_alias_given_identifier_metadata_when_input_and_output_formats_are_crossed(
) {
    // Arrange
    let sql = "WITH input(v) AS (SELECT $1::INT4) SELECT \"Parameter Alias\".v AS output FROM (SELECT v FROM input) AS \"Parameter Alias\"";
    // Act
    let batches = support_values_casts::parameters(
        "identifier-metadata-inferred_cte_alias",
        sql,
        0,
        &[55],
        &[0, 0, 0, 7],
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 23);
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
        ],
    );
}
