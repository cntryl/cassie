//! Native CTE materialization, sharing, volatility and projection witnesses.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_cte_mode_0_given_native_materialization_when_rows_are_executed() {
    // Arrange
    let sql = "WITH chosen AS MATERIALIZED (SELECT 7 AS value) SELECT value AS output FROM chosen";
    // Act
    let batches = planning::pruned_with_setup("cte-mode-0", sql, &[]);
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
fn should_preserve_cte_mode_1_given_native_materialization_when_rows_are_executed() {
    // Arrange
    let sql =
        "WITH chosen(v) AS MATERIALIZED (VALUES(7),(NULL::INTEGER)) SELECT v AS output FROM chosen";
    // Act
    let batches = planning::pruned_with_setup("cte-mode-1", sql, &[]);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55], &[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7], &[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_cte_mode_2_given_native_materialization_when_rows_are_executed() {
    // Arrange
    let sql = "WITH chosen AS MATERIALIZED (VALUES(1),(2)) SELECT first.column1+second.column1 AS output FROM chosen AS first JOIN chosen AS second ON first.column1<second.column1";
    // Act
    let batches = planning::pruned_with_setup("cte-mode-2", sql, &[]);
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
fn should_preserve_cte_mode_3_given_native_materialization_when_rows_are_executed() {
    // Arrange
    let sql = "WITH first AS MATERIALIZED (SELECT 7 AS v), second AS (SELECT v+1 AS v FROM first) SELECT v AS output FROM second";
    // Act
    let batches = planning::pruned_with_setup("cte-mode-3", sql, &[]);
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
fn should_preserve_cte_mode_4_given_native_materialization_when_rows_are_executed() {
    // Arrange
    let sql = "WITH chosen AS MATERIALIZED (SELECT 7 AS value) SELECT CASE WHEN value=7 THEN TRUE ELSE FALSE END AS output FROM chosen";
    // Act
    let batches = planning::pruned_with_setup("cte-mode-4", sql, &[]);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_preserve_cte_mode_5_given_native_materialization_when_rows_are_executed() {
    // Arrange
    let sql = "WITH RECURSIVE sequence(n) AS MATERIALIZED (SELECT CAST(1 AS BIGINT) UNION ALL SELECT n+1 FROM sequence WHERE n<3) SELECT n AS output FROM sequence ORDER BY n";
    // Act
    let batches = planning::pruned_with_setup("cte-mode-5", sql, &[]);
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
fn should_preserve_cte_mode_6_given_native_materialization_when_rows_are_executed() {
    // Arrange
    let sql =
        "WITH chosen AS NOT MATERIALIZED (SELECT 7 AS value) SELECT value AS output FROM chosen";
    // Act
    let batches = planning::pruned_with_setup("cte-mode-6", sql, &[]);
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
fn should_preserve_cte_mode_7_given_native_materialization_when_rows_are_executed() {
    // Arrange
    let sql = "WITH chosen(v) AS NOT MATERIALIZED (VALUES(7),(NULL::INTEGER)) SELECT v AS output FROM chosen";
    // Act
    let batches = planning::pruned_with_setup("cte-mode-7", sql, &[]);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55], &[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7], &[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_cte_mode_8_given_native_materialization_when_rows_are_executed() {
    // Arrange
    let sql = "WITH chosen AS NOT MATERIALIZED (VALUES(1),(2)) SELECT first.column1+second.column1 AS output FROM chosen AS first JOIN chosen AS second ON first.column1<second.column1";
    // Act
    let batches = planning::pruned_with_setup("cte-mode-8", sql, &[]);
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
fn should_preserve_cte_mode_9_given_native_materialization_when_rows_are_executed() {
    // Arrange
    let sql = "WITH first AS NOT MATERIALIZED (SELECT 7 AS v), second AS (SELECT v+1 AS v FROM first) SELECT v AS output FROM second";
    // Act
    let batches = planning::pruned_with_setup("cte-mode-9", sql, &[]);
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
fn should_preserve_cte_mode_10_given_native_materialization_when_rows_are_executed() {
    // Arrange
    let sql = "WITH chosen AS NOT MATERIALIZED (SELECT 7 AS value) SELECT CASE WHEN value=7 THEN TRUE ELSE FALSE END AS output FROM chosen";
    // Act
    let batches = planning::pruned_with_setup("cte-mode-10", sql, &[]);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_preserve_cte_mode_11_given_native_materialization_when_rows_are_executed() {
    // Arrange
    let sql = "WITH RECURSIVE sequence(n) AS NOT MATERIALIZED (SELECT CAST(1 AS BIGINT) UNION ALL SELECT n+1 FROM sequence WHERE n<3) SELECT n AS output FROM sequence ORDER BY n";
    // Act
    let batches = planning::pruned_with_setup("cte-mode-11", sql, &[]);
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
fn should_preserve_cte_mode_12_given_native_materialization_when_rows_are_executed() {
    // Arrange
    let sql = "WITH chosen AS NOT MATERIALIZED (SELECT 1/0 AS bad,1 AS good) SELECT good AS output FROM chosen";
    // Act
    let batches = planning::pruned_with_setup("cte-mode-12", sql, &[]);
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
fn should_preserve_cte_mode_13_given_native_materialization_when_rows_are_executed() {
    // Arrange
    let sql = "WITH chosen AS NOT MATERIALIZED (SELECT x,10/x AS quotient FROM (VALUES(0),(1)) AS input(x)) SELECT quotient AS output FROM chosen WHERE x<>0";
    // Act
    let batches = planning::pruned_with_setup("cte-mode-13", sql, &[]);
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
fn should_preserve_cte_mode_14_given_native_materialization_when_rows_are_executed() {
    // Arrange
    let sql = "WITH chosen AS MATERIALIZED (SELECT set_config('application_name',(current_setting('application_name')::INTEGER+1)::TEXT,FALSE) AS v) SELECT first.v::INTEGER-second.v::INTEGER AS output FROM chosen AS first JOIN chosen AS second ON TRUE";
    // Act
    let batches = planning::pruned_with_setup("cte-mode-14", sql, &["SET application_name='0'"]);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_cte_mode_15_given_native_materialization_when_rows_are_executed() {
    // Arrange
    let sql = "WITH chosen AS MATERIALIZED (SELECT x,set_config('application_name',(current_setting('application_name')::INTEGER+1)::TEXT,FALSE) AS v FROM (VALUES(0),(1)) AS input(x)) SELECT v::INTEGER AS output FROM chosen WHERE x<>0";
    // Act
    let batches = planning::pruned_with_setup("cte-mode-15", sql, &["SET application_name='0'"]);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 50]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 4]],
        ],
    );
}

#[test]
fn should_preserve_cte_mode_16_given_native_materialization_when_rows_are_executed() {
    // Arrange
    let sql = "WITH chosen AS MATERIALIZED (SELECT set_config('application_name','999',FALSE) AS v) SELECT current_setting('application_name')::INTEGER AS output";
    // Act
    let batches = planning::pruned_with_setup("cte-mode-16", sql, &["SET application_name='0'"]);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_cte_mode_17_given_native_materialization_when_rows_are_executed() {
    // Arrange
    let sql = "WITH chosen AS NOT MATERIALIZED (SELECT set_config('application_name',(current_setting('application_name')::INTEGER+1)::TEXT,FALSE) AS v) SELECT first.v::INTEGER-second.v::INTEGER AS output FROM chosen AS first JOIN chosen AS second ON TRUE";
    // Act
    let batches = planning::pruned_with_setup("cte-mode-17", sql, &["SET application_name='0'"]);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_cte_mode_18_given_native_materialization_when_rows_are_executed() {
    // Arrange
    let sql = "WITH chosen AS NOT MATERIALIZED (SELECT x,set_config('application_name',(current_setting('application_name')::INTEGER+1)::TEXT,FALSE) AS v FROM (VALUES(0),(1)) AS input(x)) SELECT v::INTEGER AS output FROM chosen WHERE x<>0";
    // Act
    let batches = planning::pruned_with_setup("cte-mode-18", sql, &["SET application_name='0'"]);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 50]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 4]],
        ],
    );
}

#[test]
fn should_preserve_cte_mode_19_given_native_materialization_when_rows_are_executed() {
    // Arrange
    let sql = "WITH chosen AS NOT MATERIALIZED (SELECT set_config('application_name','999',FALSE) AS v) SELECT current_setting('application_name')::INTEGER AS output";
    // Act
    let batches = planning::pruned_with_setup("cte-mode-19", sql, &["SET application_name='0'"]);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_cte_boundary_0_given_native_alias_and_scope_when_rows_are_executed() {
    // Arrange
    let sql = "WITH chosen(v) AS NOT MATERIALIZED (VALUES(7),(NULL::INTEGER)) SELECT * FROM chosen";
    // Act
    let batches = planning::pruned("cte-boundary-0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("v", 23, 4, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 55], &[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 7], &[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_cte_boundary_1_given_native_alias_and_scope_when_rows_are_executed() {
    // Arrange
    let sql = "WITH chosen AS NOT MATERIALIZED (SELECT 7 AS value) SELECT value+1 AS output FROM chosen ORDER BY output";
    // Act
    let batches = planning::pruned("cte-boundary-1", sql);
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
fn should_preserve_cte_boundary_2_given_native_alias_and_scope_when_rows_are_executed() {
    // Arrange
    let sql = "WITH first AS NOT MATERIALIZED (SELECT 1/0 AS bad,1 AS good), second AS (SELECT good FROM first) SELECT good AS output FROM second";
    // Act
    let batches = planning::pruned("cte-boundary-2", sql);
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
fn should_preserve_cte_boundary_3_given_native_alias_and_scope_when_rows_are_executed() {
    // Arrange
    let sql = "WITH chosen AS NOT MATERIALIZED (SELECT x,10/x AS quotient FROM (VALUES(NULL::INTEGER),(0),(1)) AS input(x) WHERE x IS NOT NULL) SELECT quotient AS output FROM chosen WHERE x<>0";
    // Act
    let batches = planning::pruned("cte-boundary-3", sql);
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
fn should_preserve_cte_boundary_4_given_native_alias_and_scope_when_rows_are_executed() {
    // Arrange
    let sql = "WITH chosen(\"Quoted value\") AS NOT MATERIALIZED (SELECT 7) SELECT \"Quoted value\" AS output FROM chosen";
    // Act
    let batches = planning::pruned("cte-boundary-4", sql);
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
fn should_preserve_cte_boundary_5_given_native_alias_and_scope_when_rows_are_executed() {
    // Arrange
    let sql = "WITH chosen AS /* barrier */ MATERIALIZED (SELECT 7 AS value) SELECT value AS output FROM chosen";
    // Act
    let batches = planning::pruned("cte-boundary-5", sql);
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
fn should_preserve_cte_boundary_6_given_native_alias_and_scope_when_rows_are_executed() {
    // Arrange
    let sql = "WITH chosen AS NOT /* inline */ MATERIALIZED (SELECT 7 AS value) SELECT value AS output FROM chosen";
    // Act
    let batches = planning::pruned("cte-boundary-6", sql);
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
fn should_preserve_cte_boundary_7_given_native_alias_and_scope_when_rows_are_executed() {
    // Arrange
    let sql = "WITH chosen AS NOT MATERIALIZED (SELECT NULL::INTEGER AS value) SELECT value AS output FROM chosen";
    // Act
    let batches = planning::pruned("cte-boundary-7", sql);
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
