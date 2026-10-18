//! Native PostgreSQL CTE dependency and scope controls at Bind and Execute.
use crate::support_bind_planning as planning;

#[test]
fn should_prune_unused_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql = "WITH unused AS (SELECT 1/0 AS n) SELECT 7::INT";
    // Act
    let batches = planning::pruned("cte-unused", sql);
    // Assert
    planning::assert_pruned(&batches, 23, "7");
}

#[test]
fn should_prune_dead_chain_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql = "WITH bad AS (SELECT 1/0 AS n), unused AS (SELECT n FROM bad) SELECT 7::INT";
    // Act
    let batches = planning::pruned("cte-dead_chain", sql);
    // Assert
    planning::assert_pruned(&batches, 23, "7");
}

#[test]
fn should_prune_dead_branch_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql = "WITH bad AS (SELECT 1/0 AS n), live AS (SELECT 7::INT AS n) SELECT n FROM live";
    // Act
    let batches = planning::pruned("cte-dead_branch", sql);
    // Assert
    planning::assert_pruned(&batches, 23, "7");
}

#[test]
fn should_prune_live_chain_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql = "WITH a AS (SELECT 7::INT AS n), b AS (SELECT n FROM a) SELECT n FROM b";
    // Act
    let batches = planning::pruned("cte-live_chain", sql);
    // Assert
    planning::assert_pruned(&batches, 23, "7");
}

#[test]
fn should_prune_aliases_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql = "WITH a(n) AS (SELECT 7::INT), unused AS (SELECT 1/0 AS n) SELECT q.n FROM a AS q";
    // Act
    let batches = planning::pruned("cte-aliases", sql);
    // Assert
    planning::assert_pruned(&batches, 23, "7");
}

#[test]
fn should_prune_nested_unused_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql = "SELECT n FROM (WITH unused AS (SELECT 1/0 AS n) SELECT 7::INT AS n) q";
    // Act
    let batches = planning::pruned("cte-nested_unused", sql);
    // Assert
    planning::assert_pruned(&batches, 23, "7");
}

#[test]
fn should_prune_shadow_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql = "WITH a AS (SELECT 1/0 AS n) SELECT n FROM (WITH a AS (SELECT 7::INT AS n) SELECT n FROM a) q";
    // Act
    let batches = planning::pruned("cte-shadow", sql);
    // Assert
    planning::assert_pruned(&batches, 23, "7");
}

#[test]
fn should_prune_shadow_dependency_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql = "WITH a AS (SELECT 1/0 AS n), b AS (WITH a AS (SELECT 7::INT AS n) SELECT n FROM a) SELECT n FROM b";
    // Act
    let batches = planning::pruned("cte-shadow_dependency", sql);
    // Assert
    planning::assert_pruned(&batches, 23, "7");
}

#[test]
fn should_prune_outer_dependency_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql =
        "WITH a AS (SELECT 7::INT AS n), b AS (SELECT n FROM a) SELECT n FROM (SELECT n FROM b) q";
    // Act
    let batches = planning::pruned("cte-outer_dependency", sql);
    // Assert
    planning::assert_pruned(&batches, 23, "7");
}

#[test]
fn should_prune_unused_recursive_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql =
        "WITH RECURSIVE a(n) AS (SELECT 1/0 UNION ALL SELECT n FROM a WHERE FALSE) SELECT 7::INT";
    // Act
    let batches = planning::pruned("cte-unused_recursive", sql);
    // Assert
    planning::assert_pruned(&batches, 23, "7");
}

#[test]
fn should_prune_exists_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql = "WITH a AS (SELECT 7::INT AS n), unused AS (SELECT 1/0 AS n) SELECT EXISTS (SELECT n FROM a)";
    // Act
    let batches = planning::pruned("cte-exists", sql);
    // Assert
    planning::assert_pruned(&batches, 16, "t");
}

#[test]
fn should_prune_exists_shadow_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql = "WITH a AS (SELECT 1/0 AS n) SELECT EXISTS (WITH a AS (SELECT 7::INT AS n) SELECT n FROM a)";
    // Act
    let batches = planning::pruned("cte-exists_shadow", sql);
    // Assert
    planning::assert_pruned(&batches, 16, "t");
}

#[test]
fn should_prune_set_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql = "WITH unused AS (SELECT 1/0 AS n), a AS (SELECT 7::INT AS n) SELECT n FROM a UNION SELECT n FROM a";
    // Act
    let batches = planning::pruned("cte-set", sql);
    // Assert
    planning::assert_pruned(&batches, 23, "7");
}

#[test]
fn should_prune_nested_dead_chain_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql = "WITH a AS (SELECT 1/0 AS n) SELECT n FROM (WITH unused AS (SELECT n FROM a) SELECT 7::INT AS n) q";
    // Act
    let batches = planning::pruned("cte-nested_dead_chain", sql);
    // Assert
    planning::assert_pruned(&batches, 23, "7");
}

#[test]
fn should_prune_dead_cast_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql = "WITH unused AS (SELECT CAST('bad' AS TEXT)::INT AS n) SELECT 7::INT";
    // Act
    let batches = planning::pruned("cte-dead_cast", sql);
    // Assert
    planning::assert_pruned(&batches, 23, "7");
}

#[test]
fn should_prune_dead_overflow_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql = "WITH unused AS (SELECT 2147483647::INT+1 AS n) SELECT 7::INT";
    // Act
    let batches = planning::pruned("cte-dead_overflow", sql);
    // Assert
    planning::assert_pruned(&batches, 23, "7");
}

#[test]
fn should_prune_shadow_body_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql = "WITH a AS (SELECT 7::INT AS n) SELECT n FROM (WITH a AS (SELECT n FROM a) SELECT n FROM a) q";
    // Act
    let batches = planning::pruned("cte-shadow_body", sql);
    // Assert
    planning::assert_pruned(&batches, 23, "7");
}

#[test]
fn should_prune_exists_case_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql = "WITH a AS (SELECT 7::INT AS n), unused AS (SELECT 1/0 AS n) SELECT (CASE WHEN EXISTS (SELECT n FROM a) THEN 7 ELSE 9 END)::INT";
    // Act
    let batches = planning::pruned("cte-exists_case", sql);
    // Assert
    planning::assert_pruned(&batches, 23, "7");
}

#[test]
fn should_prune_exists_function_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql = "WITH a AS (SELECT 7::INT AS n), unused AS (SELECT 1/0 AS n) SELECT COALESCE(EXISTS (SELECT n FROM a), FALSE)";
    // Act
    let batches = planning::pruned("cte-exists_function", sql);
    // Assert
    planning::assert_pruned(&batches, 16, "t");
}

#[test]
fn should_prune_exists_values_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql = "WITH a AS (SELECT 7::INT AS n), unused AS (SELECT 1/0 AS n) VALUES (EXISTS (SELECT n FROM a))";
    // Act
    let batches = planning::pruned("cte-exists_values", sql);
    // Assert
    planning::assert_pruned(&batches, 16, "t");
}

#[test]
fn should_prune_exists_where_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql = "WITH a AS (SELECT 7::INT AS n), unused AS (SELECT 1/0 AS n) SELECT 7::INT WHERE EXISTS (SELECT n FROM a)";
    // Act
    let batches = planning::pruned("cte-exists_where", sql);
    // Assert
    planning::assert_pruned(&batches, 23, "7");
}

#[test]
fn should_plan_direct_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let cases = [("WITH a AS (SELECT 1/0 AS n) SELECT n FROM a", "22012")];
    // Act
    let batches = planning::failures("cte-direct", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_plan_transitive_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let cases = [(
        "WITH a AS (SELECT 1/0 AS n), b AS (SELECT n FROM a) SELECT n FROM b",
        "22012",
    )];
    // Act
    let batches = planning::failures("cte-transitive", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_plan_nested_outer_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let cases = [(
        "WITH a AS (SELECT 1/0 AS n) SELECT n FROM (SELECT n FROM a) q",
        "22012",
    )];
    // Act
    let batches = planning::failures("cte-nested_outer", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_plan_exists_filter_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let cases = [(
        "WITH a AS (SELECT 1/0 AS n) SELECT EXISTS (SELECT 1 FROM a WHERE n=1)",
        "22012",
    )];
    // Act
    let batches = planning::failures("cte-exists_filter", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_plan_join_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let cases = [("WITH a AS (SELECT 1/0 AS n), b AS (SELECT 7::INT AS n) SELECT a.n FROM a JOIN b ON a.n=b.n", "22012")];
    // Act
    let batches = planning::failures("cte-join", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_plan_set_right_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let cases = [(
        "WITH a AS (SELECT 1/0 AS n) SELECT 7::INT UNION SELECT n FROM a",
        "22012",
    )];
    // Act
    let batches = planning::failures("cte-set_right", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_plan_live_cast_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let cases = [(
        "WITH a AS (SELECT CAST('bad' AS TEXT)::INT AS n) SELECT n FROM a",
        "22P02",
    )];
    // Act
    let batches = planning::failures("cte-live_cast", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_plan_live_overflow_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let cases = [(
        "WITH a AS (SELECT 2147483647::INT+1 AS n) SELECT n FROM a",
        "22003",
    )];
    // Act
    let batches = planning::failures("cte-live_overflow", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_plan_exists_case_error_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let cases = [("WITH a AS (SELECT 1/0 AS n) SELECT (CASE WHEN EXISTS (SELECT n FROM a WHERE n=1) THEN 7 ELSE 9 END)::INT", "22012")];
    // Act
    let batches = planning::failures("cte-exists_case_error", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_plan_exists_function_error_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let cases = [(
        "WITH a AS (SELECT 1/0 AS n) SELECT COALESCE(EXISTS (SELECT n FROM a WHERE n=1), FALSE)",
        "22012",
    )];
    // Act
    let batches = planning::failures("cte-exists_function_error", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_plan_exists_values_error_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let cases = [(
        "WITH a AS (SELECT 1/0 AS n) VALUES (EXISTS (SELECT n FROM a WHERE n=1))",
        "22012",
    )];
    // Act
    let batches = planning::failures("cte-exists_values_error", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_analyze_unused_relations_given_parse_flush_when_execution_would_prune_ctes() {
    // Arrange
    use crate::support_parameter_occurrence_wire::{run_script, Step};
    use crate::support_pgwire as wire;
    let sql = "WITH unused AS (SELECT n FROM missing_cte_relation) SELECT 7::INT";
    // Act
    let batches = run_script(
        "cte-unused-relation",
        vec![
            Step::ParseFlush(wire::parse_frame("p", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42P01"));
    assert_eq!(batches[1][0].0, b'Z');
}

#[test]
fn should_analyze_unused_columns_given_parse_flush_when_execution_would_prune_ctes() {
    // Arrange
    use crate::support_parameter_occurrence_wire::{run_script, Step};
    use crate::support_pgwire as wire;
    let sql = "WITH unused AS (SELECT missing_cte_column) SELECT 7::INT";
    // Act
    let batches = run_script(
        "cte-unused-column",
        vec![
            Step::ParseFlush(wire::parse_frame("p", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42703"));
    assert_eq!(batches[1][0].0, b'Z');
}

#[test]
fn should_prune_exists_local_unused_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let sql = "SELECT EXISTS (WITH unused AS (SELECT 1/0 AS n), a AS (SELECT 7::INT AS n) SELECT n FROM a)";
    // Act
    let batches = planning::pruned("cte-exists_local_unused", sql);
    // Assert
    planning::assert_pruned(&batches, 16, "t");
}

#[test]
fn should_plan_shadow_body_error_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let cases = [(
        "WITH a AS (SELECT 1/0 AS n) SELECT n FROM (WITH a AS (SELECT n FROM a) SELECT n FROM a) q",
        "22012",
    )];
    // Act
    let batches = planning::failures("cte-shadow_body_error", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_plan_exists_local_error_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let cases = [(
        "SELECT EXISTS (WITH a AS (SELECT 1/0 AS n) SELECT n FROM a WHERE n=1)",
        "22012",
    )];
    // Act
    let batches = planning::failures("cte-exists_local_error", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}

#[test]
fn should_plan_exists_local_chain_given_cte_scopes_when_binding_reaches_only_used_queries() {
    // Arrange
    let cases = [("SELECT EXISTS (WITH a AS (SELECT 1/0 AS n), b AS (SELECT n FROM a) SELECT n FROM b WHERE n=1)", "22012")];
    // Act
    let batches = planning::failures("cte-exists_local_chain", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
}
