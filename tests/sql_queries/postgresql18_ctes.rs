//! PostgreSQL 18 CTE materialization and recursive traversal contracts for #801.

use cassie::app::CassieError;
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

use crate::support_sql_fixture::{SqlFixture, sql_fixture, sql_fixture_with_config};

mod insert_snapshot_descriptor;
mod search_cycle_edge_cases;

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
fn should_expose_distinct_materialized_inline_plan_shapes() {
    // Arrange
    let fixture = repeated_reference_fixture("issue-801-materialization-plan-distinction");

    // Act
    let materialized = fixture.execute(
        "EXPLAIN WITH chosen AS MATERIALIZED (SELECT value FROM issue_801_values) SELECT value FROM chosen WHERE value = 1",
    );
    let not_materialized = fixture.execute(
        "EXPLAIN WITH chosen AS NOT MATERIALIZED (SELECT value FROM issue_801_values) SELECT value FROM chosen WHERE value = 1",
    );

    // Assert
    let materialized = materialized.expect("MATERIALIZED CTE plan");
    let not_materialized = not_materialized.expect("NOT MATERIALIZED CTE plan");
    let Value::String(materialized_plan) = &materialized.rows()[0][0] else {
        panic!("expected textual MATERIALIZED CTE plan");
    };
    let Value::String(not_materialized_plan) = &not_materialized.rows()[0][0] else {
        panic!("expected textual NOT MATERIALIZED CTE plan");
    };
    assert_ne!(
        materialized_plan, not_materialized_plan,
        "EXPLAIN must make explicit CTE materialization observable"
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
fn should_return_recursive_results_beyond_sixty_four_steps() {
    // Arrange
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.cte_recursion_depth = 64;
    let fixture = sql_fixture_with_config("issue-801-recursive-growth", &[], config);

    // Act
    let result = fixture.execute(
        "WITH RECURSIVE sequence(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM sequence WHERE n < 128) SELECT count(*), max(n) FROM sequence",
    );

    // Assert
    assert_eq!(
        result.expect("recursive CTE with 128 rows").rows(),
        &[vec![Value::Int64(128), Value::Int64(128)]]
    );
}

#[test]
fn should_limit_recursive_cycle_path_rows_by_query_result_cap() {
    // Arrange
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.max_result_rows = 5;
    let fixture = sql_fixture_with_config("issue-801-cycle-result-row-cap", &[], config);
    let query = "WITH RECURSIVE walk(node) AS (\
        SELECT 1::INT4\
        UNION ALL\
        SELECT CASE WHEN node = 32 THEN 1 ELSE node + 1 END FROM walk WHERE node <= 32\
    ) CYCLE node SET is_cycle USING path\
    SELECT node, is_cycle, path FROM walk ORDER BY node, is_cycle";
    let before = current_accounted_memory_bytes(&fixture);

    // Act
    let result = fixture.execute(query);

    // Assert
    assert!(
        matches!(
            result,
            Err(CassieError::ResourceLimit(ref message))
                if message.starts_with("query result row limit exceeded")
        ),
        "the cycle query must stop at the configured result-row cap: {result:?}"
    );
    assert_eq!(current_accounted_memory_bytes(&fixture), before);
    let snapshot = fixture.cassie.metrics();
    assert_eq!(snapshot["runtime"]["running_queries"], 0);
    assert_eq!(snapshot["runtime"]["active_operator_workers"], 0);
    assert_eq!(
        fixture.rows("SELECT 45 AS recovery"),
        vec![vec![Value::Int64(45)]]
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
fn should_order_recursive_rows_by_postgres_depth_first_search_sequence() {
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
fn should_order_depth_first_search_by_each_declared_key() {
    // Arrange
    let fixture = sql_fixture("issue-801-search-depth-first-multiple-keys", &[]);

    // Act
    let result = fixture.execute(
        "WITH RECURSIVE tree(parent, node, priority) AS (\
             VALUES (NULL::INT4, 1::INT4, 0::INT4), (1, 2, 2), (1, 3, 1), (2, 4, 1), (3, 5, 2)\
         ), walk(node, priority) AS (\
             SELECT node, priority FROM tree WHERE parent IS NULL\
             UNION ALL\
             SELECT child.node, child.priority\
             FROM tree AS child JOIN walk AS parent ON child.parent = parent.node\
         ) SEARCH DEPTH FIRST BY priority, node SET sequence\
         SELECT node FROM walk ORDER BY sequence",
    );

    // Assert
    assert_eq!(
        result.expect("multi-column depth-first SEARCH").rows(),
        &[
            vec![Value::Int64(1)],
            vec![Value::Int64(3)],
            vec![Value::Int64(5)],
            vec![Value::Int64(2)],
            vec![Value::Int64(4)],
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
fn should_detect_cycles_only_after_all_declared_keys_repeat() {
    // Arrange
    let fixture = sql_fixture("issue-801-cycle-multiple-keys", &[]);

    // Act
    let result = fixture.execute(
        "WITH RECURSIVE edges(source_node, source_label, target_node, target_label) AS (\
             VALUES (1::INT4, 'A'::TEXT, 2::INT4, 'B'::TEXT),\
                    (2::INT4, 'B'::TEXT, 1::INT4, 'C'::TEXT),\
                    (1::INT4, 'C'::TEXT, 2::INT4, 'D'::TEXT),\
                    (2::INT4, 'D'::TEXT, 1::INT4, 'A'::TEXT)\
         ), walk(node, label, depth) AS (\
             SELECT 1::INT4, 'A'::TEXT, 0::INT4\
             UNION ALL\
             SELECT edge.target_node, edge.target_label, parent.depth + 1\
             FROM edges AS edge JOIN walk AS parent\
               ON edge.source_node = parent.node AND edge.source_label = parent.label\
         ) CYCLE node, label SET is_cycle USING path\
         SELECT node, label, depth, is_cycle, path, path::TEXT FROM walk ORDER BY depth",
    );

    // Assert
    let result = result.expect("CYCLE over two declared keys");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 25, 23, 16, 2287, 25]
    );
    assert_eq!(
        result
            .rows()
            .iter()
            .map(|row| vec![
                row[0].clone(),
                row[1].clone(),
                row[2].clone(),
                row[3].clone(),
                row[5].clone(),
            ])
            .collect::<Vec<_>>(),
        vec![
            vec![
                Value::Int64(1),
                Value::String("A".to_owned()),
                Value::Int64(0),
                Value::Bool(false),
                Value::String("{\"(1,A)\"}".to_owned()),
            ],
            vec![
                Value::Int64(2),
                Value::String("B".to_owned()),
                Value::Int64(1),
                Value::Bool(false),
                Value::String("{\"(1,A)\",\"(2,B)\"}".to_owned()),
            ],
            vec![
                Value::Int64(1),
                Value::String("C".to_owned()),
                Value::Int64(2),
                Value::Bool(false),
                Value::String("{\"(1,A)\",\"(2,B)\",\"(1,C)\"}".to_owned()),
            ],
            vec![
                Value::Int64(2),
                Value::String("D".to_owned()),
                Value::Int64(3),
                Value::Bool(false),
                Value::String("{\"(1,A)\",\"(2,B)\",\"(1,C)\",\"(2,D)\"}".to_owned()),
            ],
            vec![
                Value::Int64(1),
                Value::String("A".to_owned()),
                Value::Int64(4),
                Value::Bool(true),
                Value::String("{\"(1,A)\",\"(2,B)\",\"(1,C)\",\"(2,D)\",\"(1,A)\"}".to_owned(),),
            ],
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

#[test]
fn should_feed_deleted_returning_rows_into_a_following_cte_statement() {
    // Arrange
    let fixture = sql_fixture(
        "issue-834-delete-returning-cte",
        &[
            "CREATE TABLE issue_834_source (id INT PRIMARY KEY, payload TEXT)",
            "CREATE TABLE issue_834_archive (id INT, payload TEXT)",
            "INSERT INTO issue_834_source VALUES (1, 'one'), (2, 'two')",
        ],
    );

    // Act
    let archived = fixture.execute(
        "WITH moved AS (DELETE FROM issue_834_source WHERE id = 1 RETURNING id, payload) \
         INSERT INTO issue_834_archive SELECT id, payload FROM moved RETURNING id, payload",
    );
    let source_rows = fixture.execute("SELECT id, payload FROM issue_834_source ORDER BY id");
    let archive_rows = fixture.execute("SELECT id, payload FROM issue_834_archive ORDER BY id");

    // Assert
    assert_eq!(
        archived
            .expect("consume DELETE RETURNING from the outer INSERT")
            .rows(),
        &[vec![Value::Int64(1), Value::String("one".to_owned())]]
    );
    assert_eq!(
        source_rows
            .expect("read source after the data-modifying CTE")
            .rows(),
        &[vec![Value::Int64(2), Value::String("two".to_owned())]]
    );
    assert_eq!(
        archive_rows
            .expect("read rows inserted from the CTE relation")
            .rows(),
        &[vec![Value::Int64(1), Value::String("one".to_owned())]]
    );
}

#[test]
fn should_keep_sibling_reads_on_the_data_modifying_cte_statement_snapshot() {
    // PostgreSQL 18.6 runs the sibling SELECT on the original statement
    // snapshot, while DELETE RETURNING exposes the removed row. Cassie's
    // planner currently accepts only SELECT CTE bodies, so this is #834
    // snapshot-visibility acceptance coverage whose value assertions are
    // masked until data-modifying CTE bodies are supported.
    // Arrange
    let fixture = sql_fixture(
        "issue-834-modifying-cte-snapshot",
        &[
            "CREATE TABLE issue_834_snapshot_source (id INT PRIMARY KEY, payload TEXT)",
            "INSERT INTO issue_834_snapshot_source VALUES (1, 'one'), (2, 'two')",
        ],
    );

    // Act
    let observed = fixture.execute(
        "WITH removed AS ( \
             DELETE FROM issue_834_snapshot_source WHERE id = 1 RETURNING id, payload \
         ), snapshot AS ( \
             SELECT id, payload FROM issue_834_snapshot_source \
         ) \
         SELECT 'deleted' AS source, id, payload FROM removed \
         UNION ALL \
         SELECT 'snapshot', id, payload FROM snapshot \
         ORDER BY source, id",
    );
    let remaining =
        fixture.execute("SELECT id, payload FROM issue_834_snapshot_source ORDER BY id");

    // Assert
    assert_eq!(
        observed
            .expect("data-modifying CTE and sibling SELECT statement snapshot")
            .rows(),
        &[
            vec![
                Value::String("deleted".to_owned()),
                Value::Int64(1),
                Value::String("one".to_owned()),
            ],
            vec![
                Value::String("snapshot".to_owned()),
                Value::Int64(1),
                Value::String("one".to_owned()),
            ],
            vec![
                Value::String("snapshot".to_owned()),
                Value::Int64(2),
                Value::String("two".to_owned()),
            ],
        ]
    );
    assert_eq!(
        remaining
            .expect("base table after the data-modifying CTE statement")
            .rows(),
        &[vec![Value::Int64(2), Value::String("two".to_owned())]]
    );
}

#[test]
fn should_attach_a_non_modifying_cte_to_insert() {
    // Arrange
    let fixture = sql_fixture(
        "issue-834-cte-before-insert",
        &["CREATE TABLE issue_834_insert_target (id INT PRIMARY KEY, payload TEXT)"],
    );

    // Act
    let inserted = fixture.execute(
        "WITH incoming AS (SELECT CAST(3 AS INT) AS id, CAST('three' AS TEXT) AS payload) \
         INSERT INTO issue_834_insert_target SELECT id, payload FROM incoming RETURNING id, payload",
    );
    let stored = fixture.execute("SELECT id, payload FROM issue_834_insert_target ORDER BY id");

    // Assert
    assert_eq!(
        inserted.expect("non-modifying CTE before INSERT").rows(),
        &[vec![Value::Int64(3), Value::String("three".to_owned())]]
    );
    assert_eq!(
        stored.expect("inserted CTE source row").rows(),
        &[vec![Value::Int64(3), Value::String("three".to_owned())]]
    );
}

#[test]
fn should_attach_a_non_modifying_cte_to_update() {
    // Arrange
    let fixture = sql_fixture(
        "issue-834-cte-before-update",
        &[
            "CREATE TABLE issue_834_update_target (id INT PRIMARY KEY, payload TEXT)",
            "INSERT INTO issue_834_update_target VALUES (1, 'old'), (2, 'keep')",
        ],
    );

    // Act
    let updated = fixture.execute(
        "WITH selected AS (SELECT CAST(1 AS INT) AS id) \
         UPDATE issue_834_update_target SET payload = 'changed' \
         WHERE id IN (SELECT id FROM selected) RETURNING id, payload",
    );
    let stored = fixture.execute("SELECT id, payload FROM issue_834_update_target ORDER BY id");

    // Assert
    assert_eq!(
        updated.expect("non-modifying CTE before UPDATE").rows(),
        &[vec![Value::Int64(1), Value::String("changed".to_owned())]]
    );
    assert_eq!(
        stored.expect("updated and untouched rows").rows(),
        &[
            vec![Value::Int64(1), Value::String("changed".to_owned())],
            vec![Value::Int64(2), Value::String("keep".to_owned())],
        ]
    );
}

#[test]
fn should_attach_a_non_modifying_cte_to_delete() {
    // Arrange
    let fixture = sql_fixture(
        "issue-834-cte-before-delete",
        &[
            "CREATE TABLE issue_834_delete_target (id INT PRIMARY KEY, payload TEXT)",
            "INSERT INTO issue_834_delete_target VALUES (1, 'one'), (2, 'two')",
        ],
    );

    // Act
    let deleted = fixture.execute(
        "WITH selected AS (SELECT CAST(1 AS INT) AS id) \
         DELETE FROM issue_834_delete_target \
         WHERE id IN (SELECT id FROM selected) RETURNING id, payload",
    );
    let remaining = fixture.execute("SELECT id, payload FROM issue_834_delete_target ORDER BY id");

    // Assert
    assert_eq!(
        deleted.expect("non-modifying CTE before DELETE").rows(),
        &[vec![Value::Int64(1), Value::String("one".to_owned())]]
    );
    assert_eq!(
        remaining.expect("remaining DELETE target rows").rows(),
        &[vec![Value::Int64(2), Value::String("two".to_owned())]]
    );
}

#[test]
fn should_roll_back_data_modifying_ctes_when_the_outer_insert_fails() {
    // Arrange
    let fixture = sql_fixture(
        "issue-834-modifying-cte-atomic-failure",
        &[
            "CREATE TABLE issue_834_atomic_source (id INT PRIMARY KEY)",
            "CREATE TABLE issue_834_atomic_target (id INT PRIMARY KEY)",
            "INSERT INTO issue_834_atomic_source VALUES (1)",
            "INSERT INTO issue_834_atomic_target VALUES (1)",
        ],
    );

    // Act
    let failed = fixture.execute(
        "WITH removed AS (DELETE FROM issue_834_atomic_source WHERE id = 1 RETURNING id) \
         INSERT INTO issue_834_atomic_target SELECT id FROM removed",
    );
    let source_rows = fixture.execute("SELECT id FROM issue_834_atomic_source ORDER BY id");
    let target_rows = fixture.execute("SELECT id FROM issue_834_atomic_target ORDER BY id");

    // Assert
    assert!(
        matches!(failed, Err(CassieError::UniqueViolation { .. })),
        "the outer INSERT must report its duplicate key: {failed:?}"
    );
    assert_eq!(
        source_rows.expect("source after failed statement").rows(),
        &[vec![Value::Int64(1)]]
    );
    assert_eq!(
        target_rows.expect("target after failed statement").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_attach_a_non_modifying_cte_to_merge() {
    // Arrange
    let fixture = sql_fixture(
        "issue-834-cte-before-merge",
        &[
            "CREATE TABLE issue_834_merge_target (id INT PRIMARY KEY, payload TEXT)",
            "INSERT INTO issue_834_merge_target VALUES (1, 'old')",
        ],
    );

    // Act
    let merged = fixture.execute(
        "WITH incoming(id,payload) AS (VALUES (1,'updated'),(2,'inserted')) \
         MERGE INTO issue_834_merge_target AS target \
         USING incoming AS source ON target.id = source.id \
         WHEN MATCHED THEN UPDATE SET payload = source.payload \
         WHEN NOT MATCHED THEN INSERT (id,payload) VALUES (source.id, source.payload) \
         RETURNING target.id, target.payload",
    );
    let stored = fixture.execute("SELECT id, payload FROM issue_834_merge_target ORDER BY id");

    // Assert
    let expected = &[
        vec![Value::Int64(1), Value::String("updated".to_owned())],
        vec![Value::Int64(2), Value::String("inserted".to_owned())],
    ];
    assert_eq!(
        merged.expect("non-modifying CTE before MERGE").rows(),
        expected
    );
    assert_eq!(stored.expect("MERGE target rows").rows(), expected);
}

#[test]
fn should_select_rows_from_a_merge_returning_cte() {
    // Arrange
    let fixture = sql_fixture(
        "issue-834-merge-returning-cte",
        &[
            "CREATE TABLE issue_834_merge_cte_target (id INT PRIMARY KEY, payload TEXT)",
            "INSERT INTO issue_834_merge_cte_target VALUES (1, 'old')",
            "CREATE TABLE issue_834_merge_cte_source (id INT, payload TEXT)",
            "INSERT INTO issue_834_merge_cte_source VALUES (1, 'updated'), (2, 'inserted')",
        ],
    );

    // Act
    let merged = fixture.execute(
        "WITH changed AS ( \
             MERGE INTO issue_834_merge_cte_target AS target \
             USING issue_834_merge_cte_source AS source ON target.id = source.id \
             WHEN MATCHED THEN UPDATE SET payload = source.payload \
             WHEN NOT MATCHED THEN INSERT (id,payload) VALUES (source.id, source.payload) \
             RETURNING target.id, target.payload \
         ) \
         SELECT id, payload FROM changed ORDER BY id",
    );
    let stored = fixture.execute("SELECT id, payload FROM issue_834_merge_cte_target ORDER BY id");

    // Assert
    let expected = &[
        vec![Value::Int64(1), Value::String("updated".to_owned())],
        vec![Value::Int64(2), Value::String("inserted".to_owned())],
    ];
    assert_eq!(
        merged.expect("select from MERGE RETURNING CTE").rows(),
        expected
    );
    assert_eq!(
        stored.expect("MERGE RETURNING CTE target rows").rows(),
        expected
    );
}
