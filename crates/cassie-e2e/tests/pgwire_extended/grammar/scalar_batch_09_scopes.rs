//! Native outer CTE scope survives delimited derived alias binding.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_alias_cte_scope_0_given_bound_outer_cte_when_derived_alias_is_typed() {
    // Arrange
    let sql = "WITH input(v) AS (VALUES(7)) SELECT \"ScopedAlias_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\".v AS output FROM (SELECT v FROM input) AS \"ScopedAlias_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"";
    // Act
    let batches = planning::pruned("identifier-alias-scope-0", sql);
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
fn should_preserve_alias_cte_scope_1_given_bound_outer_cte_when_derived_alias_is_typed() {
    // Arrange
    let sql = "WITH input(v) AS (VALUES(7)) SELECT \"ScopedAlias_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\".v AS output FROM (SELECT * FROM input) AS \"ScopedAlias_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"";
    // Act
    let batches = planning::pruned("identifier-alias-scope-1", sql);
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
