//! Planning evaluates reached bound constants, leaving negative bounds to execution.
use crate::support_bind_planning as planning;
use crate::support_pagination_unary as pagination;
use crate::support_unary_numeric as unary;

#[test]
fn should_preserve_negative_limit_given_bound_constants_when_planning_respects_phase_and_reachability(
) {
    // Arrange
    let sql = "SELECT 7::INT LIMIT -(1::INT)";
    // Act
    let batches = planning::pruned("page-probe-negative_limit", sql);
    // Assert
    pagination::assert_execute_failure(&batches, "2201W");
}

#[test]
fn should_preserve_negative_offset_given_bound_constants_when_planning_respects_phase_and_reachability(
) {
    // Arrange
    let sql = "SELECT 7::INT OFFSET -(1::INT)";
    // Act
    let batches = planning::pruned("page-probe-negative_offset", sql);
    // Assert
    pagination::assert_execute_failure(&batches, "2201X");
}

#[test]
fn should_preserve_parameter_limit_given_bound_constants_when_planning_respects_phase_and_reachability(
) {
    // Arrange
    let cases = [(
        "SELECT 7::INT LIMIT $1::INT + -((-32768)::SMALLINT)",
        "22003",
    )];
    // Act
    let batches = planning::failures("page-probe-parameter_limit", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_preserve_parameter_offset_given_bound_constants_when_planning_respects_phase_and_reachability(
) {
    // Arrange
    let cases = [(
        "SELECT 7::INT OFFSET $1::INT + -((-32768)::SMALLINT)",
        "22003",
    )];
    // Act
    let batches = planning::failures("page-probe-parameter_offset", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_preserve_used_cte_given_bound_constants_when_planning_respects_phase_and_reachability() {
    // Arrange
    let cases = [(
        "WITH x AS (SELECT 7::INT LIMIT -((-32768)::SMALLINT)) SELECT * FROM x",
        "22003",
    )];
    // Act
    let batches = planning::failures("page-probe-used_cte", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_preserve_used_exists_given_bound_constants_when_planning_respects_phase_and_reachability()
{
    // Arrange
    let cases = [(
        "SELECT EXISTS (SELECT 7::INT OFFSET -((-32768)::SMALLINT))",
        "22003",
    )];
    // Act
    let batches = planning::failures("page-probe-used_exists", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_preserve_unused_cte_given_bound_constants_when_planning_respects_phase_and_reachability()
{
    // Arrange
    let sql = "WITH x AS (SELECT 7::INT LIMIT -((-32768)::SMALLINT)) SELECT 7::INT";
    // Act
    let batches = planning::pruned("page-probe-unused_cte", sql);
    // Assert
    unary::assert_result(
        &batches,
        23,
        [&[0, 1, 0, 0, 0, 1, 55], &[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
    );
}

#[test]
fn should_preserve_unused_chain_given_bound_constants_when_planning_respects_phase_and_reachability(
) {
    // Arrange
    let sql = "WITH x AS (SELECT 7::INT LIMIT -((-32768)::SMALLINT)), y AS (SELECT * FROM x) SELECT 7::INT";
    // Act
    let batches = planning::pruned("page-probe-unused_chain", sql);
    // Assert
    unary::assert_result(
        &batches,
        23,
        [&[0, 1, 0, 0, 0, 1, 55], &[0, 1, 0, 0, 0, 4, 0, 0, 0, 7]],
    );
}
