//! Bound parameters prune immutable dead branches without entering cached plans.
use crate::support_bytea_cast_planning::{assert_result, run, Case};

#[test]
fn should_prune_case_true_given_bound_parameter_values_when_planning_selects_immutable_branches() {
    // Arrange
    let case = Case {
        label: "byteacasts-lazy-case_true",
        sql: "SELECT CASE WHEN $1 THEN 7::INT2 ELSE '\\x000000'::BYTEA::INT2 END AS output",
        oid: 16,
        good: [&[116], &[1]],
        bad: [Some(&[102]), Some(&[0])],
        output: [&[0, 1, 0, 0, 0, 1, 55], &[0, 1, 0, 0, 0, 2, 0, 7]],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_result(&case, &batches);
}

#[test]
fn should_prune_case_false_given_bound_parameter_values_when_planning_selects_immutable_branches() {
    // Arrange
    let case = Case {
        label: "byteacasts-lazy-case_false",
        sql: "SELECT CASE WHEN $1 THEN '\\x000000'::BYTEA::INT2 ELSE 7::INT2 END AS output",
        oid: 16,
        good: [&[102], &[0]],
        bad: [Some(&[116]), Some(&[1])],
        output: [&[0, 1, 0, 0, 0, 1, 55], &[0, 1, 0, 0, 0, 2, 0, 7]],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_result(&case, &batches);
}

#[test]
fn should_prune_coalesce_given_bound_parameter_values_when_planning_selects_immutable_branches() {
    // Arrange
    let case = Case {
        label: "byteacasts-lazy-coalesce",
        sql: "SELECT COALESCE($1::INT2, '\\x000000'::BYTEA::INT2) AS output",
        oid: 21,
        good: [&[56], &[0, 8]],
        bad: [None, None],
        output: [&[0, 1, 0, 0, 0, 1, 56], &[0, 1, 0, 0, 0, 2, 0, 8]],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_result(&case, &batches);
}
