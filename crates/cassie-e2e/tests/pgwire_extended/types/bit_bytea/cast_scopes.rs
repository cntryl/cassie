//! Pure CTE source propagation preserves aliases, pruning and descriptors.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_fold_alias_int2_given_a_single_values_cte_when_bind_planning_propagates_source_identity()
{
    // Arrange
    let cases = [(
        "WITH q(v) AS (VALUES ('\\x000000'::BYTEA)) SELECT src.v::INT2 FROM q AS src",
        "22003",
    )];
    // Act
    let batches = planning::failures("byteacasts-scope-alias_int2", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_fold_alias_int4_given_a_single_values_cte_when_bind_planning_propagates_source_identity()
{
    // Arrange
    let cases = [(
        "WITH q(v) AS (VALUES ('\\x0000000000'::BYTEA)) SELECT src.v::INT4 FROM q AS src",
        "22003",
    )];
    // Act
    let batches = planning::failures("byteacasts-scope-alias_int4", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_fold_quoted_int8_given_a_single_values_cte_when_bind_planning_propagates_source_identity()
{
    // Arrange
    let cases = [("WITH q(\"Bytes\") AS (VALUES ('\\x000000000000000000'::BYTEA)) SELECT \"Bytes\"::INT8 FROM q", "22003")];
    // Act
    let batches = planning::failures("byteacasts-scope-quoted_int8", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_preserve_wildcard_given_a_single_values_cte_when_source_propagation_is_considered() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES (1::INT2)) SELECT * FROM q";
    // Act
    let batches = planning::pruned("byteacasts-scope-wildcard", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 21, 2, -1),
        [&[&[0, 1, 0, 0, 0, 1, 49]], &[&[0, 1, 0, 0, 0, 2, 0, 1]]],
    );
}

#[test]
fn should_preserve_unused_given_a_single_values_cte_when_source_propagation_is_considered() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ('\\x000000'::BYTEA::INT2)) SELECT 7::INT AS output";
    // Act
    let batches = planning::pruned("byteacasts-scope-unused", sql);
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
