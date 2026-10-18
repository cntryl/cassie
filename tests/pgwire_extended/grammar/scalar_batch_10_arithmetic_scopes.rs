//! Native integer identity through a nonrecursive CTE dependency chain.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_arithmetic_cte_scope_given_three_dependencies_when_integer_types_are_bound() {
    // Arrange
    let sql = "WITH c0 AS NOT MATERIALIZED (SELECT 1 AS value),c1 AS NOT MATERIALIZED (SELECT value+1 AS value FROM c0),c2 AS NOT MATERIALIZED (SELECT value+1 AS value FROM c1) SELECT value AS output FROM c2";
    // Act
    let batches = planning::pruned("cte-arithmetic-scope", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 51]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 3]],
        ],
    );
}

#[test]
fn should_preserve_cte_arithmetic_given_default_smallint_when_dependencies_are_bound() {
    // Arrange
    let sql = "WITH c0 AS  (SELECT CAST(1 AS SMALLINT) AS value),c1 AS  (SELECT value+CAST(1 AS SMALLINT) AS value FROM c0),c2 AS  (SELECT value+CAST(1 AS SMALLINT) AS value FROM c1) SELECT value AS output FROM c2";
    // Act
    let batches = planning::pruned("cte-arithmetic-default_smallint", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [&[&[0, 1, 0, 0, 0, 1, 51]], &[&[0, 1, 0, 0, 0, 2, 0, 3]]],
    );
}

#[test]
fn should_preserve_cte_arithmetic_given_default_integer_when_dependencies_are_bound() {
    // Arrange
    let sql = "WITH c0 AS  (SELECT CAST(1 AS INTEGER) AS value),c1 AS  (SELECT value+CAST(1 AS INTEGER) AS value FROM c0),c2 AS  (SELECT value+CAST(1 AS INTEGER) AS value FROM c1) SELECT value AS output FROM c2";
    // Act
    let batches = planning::pruned("cte-arithmetic-default_integer", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 51]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 3]],
        ],
    );
}

#[test]
fn should_preserve_cte_arithmetic_given_default_bigint_when_dependencies_are_bound() {
    // Arrange
    let sql = "WITH c0 AS  (SELECT CAST(1 AS BIGINT) AS value),c1 AS  (SELECT value+CAST(1 AS BIGINT) AS value FROM c0),c2 AS  (SELECT value+CAST(1 AS BIGINT) AS value FROM c1) SELECT value AS output FROM c2";
    // Act
    let batches = planning::pruned("cte-arithmetic-default_bigint", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 51]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 3]],
        ],
    );
}

#[test]
fn should_preserve_cte_arithmetic_given_materialized_smallint_when_dependencies_are_bound() {
    // Arrange
    let sql = "WITH c0 AS MATERIALIZED (SELECT CAST(1 AS SMALLINT) AS value),c1 AS MATERIALIZED (SELECT value+CAST(1 AS SMALLINT) AS value FROM c0),c2 AS MATERIALIZED (SELECT value+CAST(1 AS SMALLINT) AS value FROM c1) SELECT value AS output FROM c2";
    // Act
    let batches = planning::pruned("cte-arithmetic-materialized_smallint", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [&[&[0, 1, 0, 0, 0, 1, 51]], &[&[0, 1, 0, 0, 0, 2, 0, 3]]],
    );
}

#[test]
fn should_preserve_cte_arithmetic_given_materialized_integer_when_dependencies_are_bound() {
    // Arrange
    let sql = "WITH c0 AS MATERIALIZED (SELECT CAST(1 AS INTEGER) AS value),c1 AS MATERIALIZED (SELECT value+CAST(1 AS INTEGER) AS value FROM c0),c2 AS MATERIALIZED (SELECT value+CAST(1 AS INTEGER) AS value FROM c1) SELECT value AS output FROM c2";
    // Act
    let batches = planning::pruned("cte-arithmetic-materialized_integer", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 51]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 3]],
        ],
    );
}

#[test]
fn should_preserve_cte_arithmetic_given_materialized_bigint_when_dependencies_are_bound() {
    // Arrange
    let sql = "WITH c0 AS MATERIALIZED (SELECT CAST(1 AS BIGINT) AS value),c1 AS MATERIALIZED (SELECT value+CAST(1 AS BIGINT) AS value FROM c0),c2 AS MATERIALIZED (SELECT value+CAST(1 AS BIGINT) AS value FROM c1) SELECT value AS output FROM c2";
    // Act
    let batches = planning::pruned("cte-arithmetic-materialized_bigint", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 51]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 3]],
        ],
    );
}

#[test]
fn should_preserve_cte_arithmetic_given_not_materialized_smallint_when_dependencies_are_bound() {
    // Arrange
    let sql = "WITH c0 AS NOT MATERIALIZED (SELECT CAST(1 AS SMALLINT) AS value),c1 AS NOT MATERIALIZED (SELECT value+CAST(1 AS SMALLINT) AS value FROM c0),c2 AS NOT MATERIALIZED (SELECT value+CAST(1 AS SMALLINT) AS value FROM c1) SELECT value AS output FROM c2";
    // Act
    let batches = planning::pruned("cte-arithmetic-not_materialized_smallint", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [&[&[0, 1, 0, 0, 0, 1, 51]], &[&[0, 1, 0, 0, 0, 2, 0, 3]]],
    );
}

#[test]
fn should_preserve_cte_arithmetic_given_not_materialized_integer_when_dependencies_are_bound() {
    // Arrange
    let sql = "WITH c0 AS NOT MATERIALIZED (SELECT CAST(1 AS INTEGER) AS value),c1 AS NOT MATERIALIZED (SELECT value+CAST(1 AS INTEGER) AS value FROM c0),c2 AS NOT MATERIALIZED (SELECT value+CAST(1 AS INTEGER) AS value FROM c1) SELECT value AS output FROM c2";
    // Act
    let batches = planning::pruned("cte-arithmetic-not_materialized_integer", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 51]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 3]],
        ],
    );
}

#[test]
fn should_preserve_cte_arithmetic_given_not_materialized_bigint_when_dependencies_are_bound() {
    // Arrange
    let sql = "WITH c0 AS NOT MATERIALIZED (SELECT CAST(1 AS BIGINT) AS value),c1 AS NOT MATERIALIZED (SELECT value+CAST(1 AS BIGINT) AS value FROM c0),c2 AS NOT MATERIALIZED (SELECT value+CAST(1 AS BIGINT) AS value FROM c1) SELECT value AS output FROM c2";
    // Act
    let batches = planning::pruned("cte-arithmetic-not_materialized_bigint", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 51]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 3]],
        ],
    );
}

#[test]
fn should_preserve_recursive_anchor_given_default_smallint_when_integer_signatures_are_bound() {
    // Arrange
    let sql = "WITH RECURSIVE sequence(n) AS  (SELECT CAST(1 AS SMALLINT) UNION ALL SELECT n+CAST(1 AS SMALLINT) FROM sequence WHERE n<3) SELECT n AS output FROM sequence ORDER BY n";
    // Act
    let batches = planning::pruned("cte-recursive-default_smallint", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 50],
                &[0, 1, 0, 0, 0, 1, 51],
            ],
            &[
                &[0, 1, 0, 0, 0, 2, 0, 1],
                &[0, 1, 0, 0, 0, 2, 0, 2],
                &[0, 1, 0, 0, 0, 2, 0, 3],
            ],
        ],
    );
}

#[test]
fn should_preserve_recursive_anchor_given_default_integer_when_integer_signatures_are_bound() {
    // Arrange
    let sql = "WITH RECURSIVE sequence(n) AS  (SELECT CAST(1 AS INTEGER) UNION ALL SELECT n+CAST(1 AS INTEGER) FROM sequence WHERE n<3) SELECT n AS output FROM sequence ORDER BY n";
    // Act
    let batches = planning::pruned("cte-recursive-default_integer", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 50],
                &[0, 1, 0, 0, 0, 1, 51],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 2],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 3],
            ],
        ],
    );
}

#[test]
fn should_preserve_recursive_anchor_given_default_bigint_when_integer_signatures_are_bound() {
    // Arrange
    let sql = "WITH RECURSIVE sequence(n) AS  (SELECT CAST(1 AS BIGINT) UNION ALL SELECT n+CAST(1 AS BIGINT) FROM sequence WHERE n<3) SELECT n AS output FROM sequence ORDER BY n";
    // Act
    let batches = planning::pruned("cte-recursive-default_bigint", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 50],
                &[0, 1, 0, 0, 0, 1, 51],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 2],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 3],
            ],
        ],
    );
}

#[test]
fn should_preserve_recursive_anchor_given_materialized_smallint_when_integer_signatures_are_bound()
{
    // Arrange
    let sql = "WITH RECURSIVE sequence(n) AS MATERIALIZED (SELECT CAST(1 AS SMALLINT) UNION ALL SELECT n+CAST(1 AS SMALLINT) FROM sequence WHERE n<3) SELECT n AS output FROM sequence ORDER BY n";
    // Act
    let batches = planning::pruned("cte-recursive-materialized_smallint", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 50],
                &[0, 1, 0, 0, 0, 1, 51],
            ],
            &[
                &[0, 1, 0, 0, 0, 2, 0, 1],
                &[0, 1, 0, 0, 0, 2, 0, 2],
                &[0, 1, 0, 0, 0, 2, 0, 3],
            ],
        ],
    );
}

#[test]
fn should_preserve_recursive_anchor_given_materialized_integer_when_integer_signatures_are_bound() {
    // Arrange
    let sql = "WITH RECURSIVE sequence(n) AS MATERIALIZED (SELECT CAST(1 AS INTEGER) UNION ALL SELECT n+CAST(1 AS INTEGER) FROM sequence WHERE n<3) SELECT n AS output FROM sequence ORDER BY n";
    // Act
    let batches = planning::pruned("cte-recursive-materialized_integer", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 50],
                &[0, 1, 0, 0, 0, 1, 51],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 2],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 3],
            ],
        ],
    );
}

#[test]
fn should_preserve_recursive_anchor_given_materialized_bigint_when_integer_signatures_are_bound() {
    // Arrange
    let sql = "WITH RECURSIVE sequence(n) AS MATERIALIZED (SELECT CAST(1 AS BIGINT) UNION ALL SELECT n+CAST(1 AS BIGINT) FROM sequence WHERE n<3) SELECT n AS output FROM sequence ORDER BY n";
    // Act
    let batches = planning::pruned("cte-recursive-materialized_bigint", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 50],
                &[0, 1, 0, 0, 0, 1, 51],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 2],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 3],
            ],
        ],
    );
}

#[test]
fn should_preserve_recursive_anchor_given_not_materialized_smallint_when_integer_signatures_are_bound(
) {
    // Arrange
    let sql = "WITH RECURSIVE sequence(n) AS NOT MATERIALIZED (SELECT CAST(1 AS SMALLINT) UNION ALL SELECT n+CAST(1 AS SMALLINT) FROM sequence WHERE n<3) SELECT n AS output FROM sequence ORDER BY n";
    // Act
    let batches = planning::pruned("cte-recursive-not_materialized_smallint", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 21, 2, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 50],
                &[0, 1, 0, 0, 0, 1, 51],
            ],
            &[
                &[0, 1, 0, 0, 0, 2, 0, 1],
                &[0, 1, 0, 0, 0, 2, 0, 2],
                &[0, 1, 0, 0, 0, 2, 0, 3],
            ],
        ],
    );
}

#[test]
fn should_preserve_recursive_anchor_given_not_materialized_integer_when_integer_signatures_are_bound(
) {
    // Arrange
    let sql = "WITH RECURSIVE sequence(n) AS NOT MATERIALIZED (SELECT CAST(1 AS INTEGER) UNION ALL SELECT n+CAST(1 AS INTEGER) FROM sequence WHERE n<3) SELECT n AS output FROM sequence ORDER BY n";
    // Act
    let batches = planning::pruned("cte-recursive-not_materialized_integer", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 50],
                &[0, 1, 0, 0, 0, 1, 51],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 2],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 3],
            ],
        ],
    );
}

#[test]
fn should_preserve_recursive_anchor_given_not_materialized_bigint_when_integer_signatures_are_bound(
) {
    // Arrange
    let sql = "WITH RECURSIVE sequence(n) AS NOT MATERIALIZED (SELECT CAST(1 AS BIGINT) UNION ALL SELECT n+CAST(1 AS BIGINT) FROM sequence WHERE n<3) SELECT n AS output FROM sequence ORDER BY n";
    // Act
    let batches = planning::pruned("cte-recursive-not_materialized_bigint", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 50],
                &[0, 1, 0, 0, 0, 1, 51],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 2],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 3],
            ],
        ],
    );
}

#[test]
fn should_preserve_recursive_outer_scope_given_mode_0_when_anchor_columns_have_quoted_aliases() {
    // Arrange
    let sql = "WITH RECURSIVE seed(value,label) AS (VALUES(CAST(1 AS BIGINT),'step')), sequence(\"Counter\",\"Label\") AS MATERIALIZED (SELECT value,label FROM seed UNION ALL SELECT \"Counter\"+1,\"Label\" FROM sequence WHERE \"Counter\"<3) SELECT \"Counter\" AS output FROM sequence WHERE \"Label\"='step' ORDER BY \"Counter\"";
    // Act
    let batches = planning::pruned("cte-anchor-scope-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 50],
                &[0, 1, 0, 0, 0, 1, 51],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 2],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 3],
            ],
        ],
    );
}

#[test]
fn should_preserve_recursive_outer_scope_given_mode_1_when_anchor_columns_have_quoted_aliases() {
    // Arrange
    let sql = "WITH RECURSIVE seed(value,label) AS (VALUES(CAST(1 AS BIGINT),'step')), sequence(\"Counter\",\"Label\") AS NOT MATERIALIZED (SELECT value,label FROM seed UNION ALL SELECT \"Counter\"+1,\"Label\" FROM sequence WHERE \"Counter\"<3) SELECT \"Counter\" AS output FROM sequence WHERE \"Label\"='step' ORDER BY \"Counter\"";
    // Act
    let batches = planning::pruned("cte-anchor-scope-1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 50],
                &[0, 1, 0, 0, 0, 1, 51],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 2],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 3],
            ],
        ],
    );
}
