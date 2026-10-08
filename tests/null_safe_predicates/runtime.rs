use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_preserve_null_safe_join_multiplicity() {
    // Arrange
    let fixture = sql_fixture(
        "null_safe_join",
        &[
            "CREATE TABLE lhs (id BIGINT, n BIGINT)",
            "CREATE TABLE rhs (id BIGINT, n BIGINT)",
            "INSERT INTO lhs VALUES (1,NULL),(2,7),(3,NULL)",
            "INSERT INTO rhs VALUES (10,NULL),(20,7),(30,NULL)",
        ],
    );
    // Act
    let result = fixture.execute("SELECT l.id,r.id FROM lhs l JOIN rhs r ON l.n IS NOT DISTINCT FROM r.n ORDER BY l.id,r.id").expect("null-safe scalar join");
    // Assert
    let expected = [(1, 10), (1, 30), (2, 20), (3, 10), (3, 30)]
        .map(|(l, r)| vec![Value::Int64(l), Value::Int64(r)]);
    assert_eq!(result.rows, expected);
    assert_eq!(
        result
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "id"]
    );
}

#[test]
fn should_preserve_null_safe_dml_predicates() {
    // Arrange
    let fixture = sql_fixture(
        "null_safe_dml",
        &[
            "CREATE TABLE dml_source (id BIGINT,n BIGINT,same BOOLEAN)",
            "INSERT INTO dml_source VALUES (1,NULL,FALSE),(2,7,FALSE)",
        ],
    );
    // Act
    let update = fixture.execute("UPDATE dml_source SET same=n IS NOT DISTINCT FROM NULL WHERE id IS NOT DISTINCT FROM 1 RETURNING id,same").expect("null-safe assignment and filter");
    let delete = fixture
        .execute("DELETE FROM dml_source WHERE n IS DISTINCT FROM 7 RETURNING id")
        .expect("null-safe delete filter");
    // Assert
    assert_eq!(update.rows, vec![vec![Value::Int64(1), Value::Bool(true)]]);
    assert_eq!(delete.rows, vec![vec![Value::Int64(1)]]);
    assert_eq!(
        fixture.rows("SELECT id FROM dml_source ORDER BY id"),
        vec![vec![Value::Int64(2)]]
    );
}

#[test]
fn should_preserve_null_safe_residual_results_with_an_index() {
    // Arrange
    let fixture = sql_fixture(
        "null_safe_residual",
        &[
            "CREATE TABLE indexed_source (id BIGINT,n BIGINT)",
            "INSERT INTO indexed_source VALUES (1,NULL),(2,7),(3,NULL),(4,8)",
            "CREATE INDEX indexed_n ON indexed_source(n)",
        ],
    );
    // Act
    let result = fixture
        .execute("SELECT id FROM indexed_source WHERE n IS DISTINCT FROM 7 ORDER BY id")
        .expect("residual NULL-safe scan");
    // Assert
    assert_eq!(
        result.rows,
        vec![
            vec![Value::Int64(1)],
            vec![Value::Int64(3)],
            vec![Value::Int64(4)]
        ]
    );
}

#[test]
fn should_preserve_null_safe_transaction_overlay_visibility() {
    // Arrange
    let fixture = sql_fixture(
        "null_safe_overlay",
        &[
            "CREATE TABLE overlay_source (id BIGINT,n BIGINT)",
            "INSERT INTO overlay_source VALUES(1,7)",
            "BEGIN",
            "INSERT INTO overlay_source VALUES(2,NULL)",
        ],
    );
    // Act
    let staged =
        fixture.rows("SELECT id FROM overlay_source WHERE n IS DISTINCT FROM 7 ORDER BY id");
    fixture.execute("ROLLBACK").expect("rollback staged row");
    // Assert
    assert_eq!(staged, vec![vec![Value::Int64(2)]]);
    assert_eq!(
        fixture.rows("SELECT id FROM overlay_source WHERE n IS DISTINCT FROM 7"),
        Vec::<Vec<Value>>::new()
    );
    let snapshot = fixture.cassie.metrics();
    assert_eq!(snapshot["query"]["current_accounted_memory_bytes"], 0);
    assert_eq!(snapshot["runtime"]["running_queries"], 0);
    assert_eq!(snapshot["runtime"]["active_operator_workers"], 0);
}

#[test]
fn should_preserve_null_safe_query_cancellation() {
    // Arrange
    let fixture = sql_fixture("null_safe_cancel", &[]);
    let cancellation = cassie::runtime::QueryCancellationHandle::new();
    cancellation.cancel();
    // Act
    let result = fixture.cassie.execute_sql_with_cancellation(
        &fixture.session,
        "SELECT NULL IS NOT DISTINCT FROM NULL AS same",
        vec![],
        &cancellation,
    );
    // Assert
    assert!(
        matches!(result, Err(cassie::app::CassieError::QueryCancelled)),
        "should_preserve_null_safe_query_cancellation should report QueryCancelled"
    );
    let snapshot = fixture.cassie.metrics();
    assert_eq!(snapshot["query"]["current_accounted_memory_bytes"], 0);
    assert_eq!(snapshot["runtime"]["running_queries"], 0);
    assert_eq!(snapshot["runtime"]["active_operator_workers"], 0);
}

#[test]
fn should_preserve_null_safe_scan_memory_denial() {
    // Arrange
    let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("config");
    config.limits.query_memory_budget_bytes = 4096;
    config.limits.execution_result_cache_enabled =
        cassie::config::ExecutionResultCacheEnabled::disabled();
    let fixture = crate::support_sql_fixture::sql_fixture_with_config(
        "null_safe_budget",
        &["CREATE TABLE budget_source(id BIGINT,payload TEXT)"],
        config,
    );
    let collection =
        crate::support_sql::canonical_test_collection(&fixture.cassie, "budget_source");
    fixture
        .cassie
        .midge
        .put_fresh_documents(
            &collection,
            (0..16)
                .map(|id| {
                    (
                        Some(format!("doc-{id}")),
                        serde_json::json!({"id":id,"payload":"x".repeat(4096)}),
                    )
                })
                .collect(),
        )
        .expect("seed outside query controls");
    // Act
    let result = fixture
        .execute("SELECT id FROM budget_source WHERE payload IS DISTINCT FROM NULL ORDER BY id");
    // Assert
    assert!(
        matches!(result, Err(cassie::app::CassieError::ResourceLimit(_))),
        "should_preserve_null_safe_scan_memory_denial should report ResourceLimit"
    );
    let snapshot = fixture.cassie.metrics();
    assert_eq!(snapshot["query"]["current_accounted_memory_bytes"], 0);
    assert_eq!(snapshot["runtime"]["running_queries"], 0);
    assert_eq!(snapshot["runtime"]["active_operator_workers"], 0);
}

#[test]
fn should_preserve_null_safe_keyset_residual_limit() {
    // Arrange
    let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("config");
    config.limits.execution_result_cache_enabled =
        cassie::config::ExecutionResultCacheEnabled::disabled();
    let fixture = crate::support_sql_fixture::sql_fixture_with_config(
        "null_safe_keyset",
        &[
            "CREATE TABLE keyset_source(id BIGINT,n BIGINT)",
            "INSERT INTO keyset_source VALUES(1,7),(2,8),(3,NULL),(4,7),(5,NULL),(6,8)",
            "CREATE INDEX keyset_id ON keyset_source(id)",
        ],
        config,
    );
    let before = fixture.cassie.metrics();
    // Act
    let result = fixture.execute("SELECT id FROM keyset_source WHERE id>0 AND n IS NOT DISTINCT FROM NULL ORDER BY id LIMIT 2").expect("post-key residual must precede LIMIT");
    // Assert
    assert_eq!(
        result.rows,
        vec![vec![Value::Int64(3)], vec![Value::Int64(5)]]
    );
    let after = fixture.cassie.metrics();
    eprintln!(
        "null-safe keyset-context before={} after={}",
        before["read_paths"], after["read_paths"]
    );
    assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
}
