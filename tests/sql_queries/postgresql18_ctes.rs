//! PostgreSQL 18 CTE materialization and recursive traversal contracts for #801.

use cassie::app::CassieError;
use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn repeated_reference_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE issue_801_values (value BIGINT)",
            "INSERT INTO issue_801_values VALUES (1), (2), (3)",
        ],
    )
}

fn tree_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE issue_801_tree (parent BIGINT, node BIGINT)",
            "INSERT INTO issue_801_tree VALUES (NULL, 1), (1, 2), (1, 3), (2, 4), (3, 5)",
        ],
    )
}

fn current_accounted_memory_bytes(fixture: &SqlFixture) -> u64 {
    fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"]
        .as_u64()
        .expect("query memory metric")
}

#[test]
fn should_share_materialized_cte_rows_across_references() {
    // Arrange
    let fixture = repeated_reference_fixture("issue-801-materialized-shared-rows");

    // Act
    let result = fixture.execute(
        "WITH chosen AS MATERIALIZED (SELECT value FROM issue_801_values) SELECT first.value, second.value FROM chosen AS first JOIN chosen AS second ON first.value < second.value ORDER BY first.value, second.value",
    );

    // Assert
    assert_eq!(
        result.expect("multiply referenced MATERIALIZED CTE").rows(),
        &[
            vec![Value::Int64(1), Value::Int64(2)],
            vec![Value::Int64(1), Value::Int64(3)],
            vec![Value::Int64(2), Value::Int64(3)],
        ]
    );
}

#[test]
fn should_preserve_results_for_multiple_not_materialized_references() {
    // Arrange
    let fixture = repeated_reference_fixture("issue-801-not-materialized-references");

    // Act
    let result = fixture.execute(
        "WITH chosen AS NOT MATERIALIZED (SELECT value FROM issue_801_values) SELECT first.value, second.value FROM chosen AS first JOIN chosen AS second ON first.value < second.value ORDER BY first.value, second.value",
    );

    // Assert
    assert_eq!(
        result
            .expect("multiply referenced NOT MATERIALIZED CTE")
            .rows(),
        &[
            vec![Value::Int64(1), Value::Int64(2)],
            vec![Value::Int64(1), Value::Int64(3)],
            vec![Value::Int64(2), Value::Int64(3)],
        ]
    );
}

#[test]
fn should_bind_parameters_inside_materialized_ctes() {
    // Arrange
    let fixture = sql_fixture("issue-801-materialized-parameters", &[]);

    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        "WITH chosen AS MATERIALIZED (SELECT CAST($1 AS BIGINT) AS value) SELECT value FROM chosen WHERE value = CAST($2 AS BIGINT)",
        vec![Value::Int64(42), Value::Int64(42)],
    );

    // Assert
    assert_eq!(
        result.expect("typed parameters in MATERIALIZED CTE").rows(),
        &[vec![Value::Int64(42)]]
    );
}

#[test]
fn should_preserve_recursive_results_with_materialized_modifier() {
    // Arrange
    let fixture = sql_fixture("issue-801-recursive-materialized", &[]);

    // Act
    let result = fixture.execute(
        "WITH RECURSIVE sequence(n) AS MATERIALIZED (SELECT CAST(1 AS BIGINT) UNION ALL SELECT n + 1 FROM sequence WHERE n < 3) SELECT n FROM sequence ORDER BY n",
    );

    // Assert
    assert_eq!(
        result.expect("recursive MATERIALIZED CTE").rows(),
        &[
            vec![Value::Int64(1)],
            vec![Value::Int64(2)],
            vec![Value::Int64(3)],
        ]
    );
}

#[test]
fn should_preserve_recursive_results_when_not_materialized_is_ignored() {
    // Arrange
    let fixture = sql_fixture("issue-801-recursive-not-materialized", &[]);

    // Act
    let result = fixture.execute(
        "WITH RECURSIVE sequence(n) AS NOT MATERIALIZED (SELECT CAST(1 AS BIGINT) UNION ALL SELECT n + 1 FROM sequence WHERE n < 3) SELECT n FROM sequence ORDER BY n",
    );

    // Assert
    assert_eq!(
        result.expect("recursive NOT MATERIALIZED CTE").rows(),
        &[
            vec![Value::Int64(1)],
            vec![Value::Int64(2)],
            vec![Value::Int64(3)],
        ]
    );
}

#[test]
fn should_order_recursive_rows_by_depth_first_search_sequence() {
    // Arrange
    let fixture = tree_fixture("issue-801-search-depth-first");

    // Act
    let result = fixture.execute(
        "WITH RECURSIVE walk(node) AS (SELECT node FROM issue_801_tree WHERE parent IS NULL UNION ALL SELECT child.node FROM issue_801_tree AS child JOIN walk AS parent ON child.parent = parent.node) SEARCH DEPTH FIRST BY node SET sequence SELECT node FROM walk ORDER BY sequence",
    );

    // Assert
    assert_eq!(
        result.expect("depth-first SEARCH ordering").rows(),
        &[
            vec![Value::Int64(1)],
            vec![Value::Int64(2)],
            vec![Value::Int64(4)],
            vec![Value::Int64(3)],
            vec![Value::Int64(5)],
        ]
    );
}

#[test]
fn should_order_recursive_rows_by_breadth_first_search_sequence() {
    // Arrange
    let fixture = tree_fixture("issue-801-search-breadth-first");

    // Act
    let result = fixture.execute(
        "WITH RECURSIVE walk(node) AS (SELECT node FROM issue_801_tree WHERE parent IS NULL UNION ALL SELECT child.node FROM issue_801_tree AS child JOIN walk AS parent ON child.parent = parent.node) SEARCH BREADTH FIRST BY node SET sequence SELECT node FROM walk ORDER BY sequence",
    );

    // Assert
    assert_eq!(
        result.expect("breadth-first SEARCH ordering").rows(),
        &[
            vec![Value::Int64(1)],
            vec![Value::Int64(2)],
            vec![Value::Int64(3)],
            vec![Value::Int64(4)],
            vec![Value::Int64(5)],
        ]
    );
}

#[test]
fn should_stop_recursion_after_marking_cycle_rows() {
    // Arrange
    let fixture = sql_fixture(
        "issue-801-cycle-detection",
        &[
            "CREATE TABLE issue_801_edges (source BIGINT, target BIGINT)",
            "INSERT INTO issue_801_edges VALUES (1, 2), (2, 3), (3, 1)",
        ],
    );

    // Act
    let result = fixture.execute(
        "WITH RECURSIVE walk(node) AS (SELECT CAST(1 AS BIGINT) UNION ALL SELECT edge.target FROM issue_801_edges AS edge JOIN walk AS parent ON edge.source = parent.node) CYCLE node SET is_cycle USING path SELECT node, is_cycle FROM walk ORDER BY node, is_cycle",
    );

    // Assert
    assert_eq!(
        result.expect("CYCLE marks repeated path node").rows(),
        &[
            vec![Value::Int64(1), Value::Bool(false)],
            vec![Value::Int64(1), Value::Bool(true)],
            vec![Value::Int64(2), Value::Bool(false)],
            vec![Value::Int64(3), Value::Bool(false)],
        ]
    );
}

#[test]
fn should_emit_custom_cycle_mark_values() {
    // Arrange
    let fixture = sql_fixture(
        "issue-801-cycle-custom-mark",
        &[
            "CREATE TABLE issue_801_mark_edges (source BIGINT, target BIGINT)",
            "INSERT INTO issue_801_mark_edges VALUES (1, 2), (2, 1)",
        ],
    );

    // Act
    let result = fixture.execute(
        "WITH RECURSIVE walk(node) AS (SELECT CAST(1 AS BIGINT) UNION ALL SELECT edge.target FROM issue_801_mark_edges AS edge JOIN walk AS parent ON edge.source = parent.node) CYCLE node SET marker TO 'cycle' DEFAULT 'clear' USING path SELECT node, marker FROM walk ORDER BY node, marker",
    );

    // Assert
    assert_eq!(
        result.expect("custom CYCLE marks").rows(),
        &[
            vec![Value::Int64(1), Value::String("clear".to_owned())],
            vec![Value::Int64(1), Value::String("cycle".to_owned())],
            vec![Value::Int64(2), Value::String("clear".to_owned())],
        ]
    );
}

#[test]
fn should_release_materialized_cte_memory_when_the_result_is_dropped() {
    // Arrange
    let fixture = repeated_reference_fixture("issue-801-materialized-memory-release");
    let current_before = current_accounted_memory_bytes(&fixture);

    // Act
    let result = fixture
        .execute(
            "WITH chosen AS MATERIALIZED (SELECT value FROM issue_801_values) SELECT value FROM chosen ORDER BY value",
        )
        .expect("materialized query result");
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Int64(1)],
            vec![Value::Int64(2)],
            vec![Value::Int64(3)],
        ]
    );
    drop(result);

    // Assert
    assert_eq!(current_accounted_memory_bytes(&fixture), current_before);
}

#[test]
fn should_release_materialized_cte_memory_after_a_consumer_error() {
    // Arrange
    let fixture = repeated_reference_fixture("issue-801-materialized-error-release");
    let current_before = current_accounted_memory_bytes(&fixture);

    // Act
    let result = fixture.execute(
        "WITH chosen AS MATERIALIZED (SELECT value FROM issue_801_values) SELECT value / (value - value) FROM chosen",
    );

    // Assert
    assert!(
        matches!(
            result,
            Err(CassieError::Execution(ref message)) if message == "division by zero"
        ),
        "consumer arithmetic should fail after reading the materialized CTE: {result:?}"
    );
    assert_eq!(current_accounted_memory_bytes(&fixture), current_before);
    assert_eq!(
        fixture.rows("SELECT value FROM issue_801_values ORDER BY value"),
        &[
            vec![Value::Int64(1)],
            vec![Value::Int64(2)],
            vec![Value::Int64(3)],
        ]
    );
}
