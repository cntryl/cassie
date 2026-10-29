use super::*;

async fn query_partial_index_rows(
    client: &tokio_postgres::Client,
    predicate: &str,
    ordering: &str,
) -> Vec<(i32, Option<i32>, bool, String)> {
    let sql = format!(
        "SELECT id, key_value, active, payload \
         FROM compat_partial_index_equivalence_rows \
         WHERE {predicate} ORDER BY {ordering}"
    );
    client
        .query(&sql, &[])
        .await
        .expect("read rows through the chosen partial-index query predicate")
        .iter()
        .map(|row| {
            (
                row.get::<_, i32>(0),
                row.get::<_, Option<i32>>(1),
                row.get::<_, bool>(2),
                row.get::<_, String>(3),
            )
        })
        .collect()
}

async fn query_active_partial_index_rows(client: &tokio_postgres::Client) -> Vec<(i32, String)> {
    let mut rows = client
        .query(
            "SELECT key_value, payload \
             FROM compat_partial_index_equivalence_rows \
             WHERE active AND key_value = 30",
            &[],
        )
        .await
        .expect("read matching partial-index entries")
        .iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect::<Vec<_>>();
    rows.sort_unstable();
    rows
}

async fn active_partial_index_plan(client: &tokio_postgres::Client) -> String {
    client
        .query_one(
            "EXPLAIN SELECT key_value, payload \
             FROM compat_partial_index_equivalence_rows \
             WHERE active AND key_value = 30",
            &[],
        )
        .await
        .expect("explain an eligible partial-index lookup")
        .get(0)
}

async fn query_moved_partial_index_rows(
    client: &tokio_postgres::Client,
    relation: &str,
) -> Vec<(i32, String)> {
    let sql = format!(
        "SELECT key_value, payload FROM {relation} \
         WHERE active AND key_value = 30 ORDER BY payload"
    );
    client
        .query(&sql, &[])
        .await
        .expect("read rows through the moved partial index")
        .iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect()
}

async fn moved_partial_index_plan(client: &tokio_postgres::Client, relation: &str) -> String {
    let sql = format!(
        "EXPLAIN SELECT key_value, payload FROM {relation} \
         WHERE active AND key_value = 30"
    );
    client
        .query_one(&sql, &[])
        .await
        .expect("explain the moved partial-index lookup")
        .get(0)
}

async fn create_active_partial_index(client: &tokio_postgres::Client) {
    client
        .batch_execute(
            "CREATE INDEX compat_partial_index_equivalence_idx \
             ON compat_partial_index_equivalence_rows (key_value) \
             INCLUDE (payload) WHERE active AND key_value = 30",
        )
        .await
        .expect("create the partial covering index");
}

async fn verify_partial_index_scan_equivalence(
    client: &tokio_postgres::Client,
    indexed_rows: &[(i32, String)],
) {
    client
        .batch_execute("DROP INDEX compat_partial_index_equivalence_idx")
        .await
        .expect("drop the index to obtain an independent scan result");
    assert_eq!(
        query_active_partial_index_rows(client).await,
        indexed_rows,
        "the independent scan should match the indexed read"
    );
}

async fn create_partial_index_write_fixture(client: &tokio_postgres::Client) {
    execute_statements(
        client,
        &[
            "CREATE TABLE compat_partial_index_equivalence_rows \
               (id INT PRIMARY KEY, key_value INT, active BOOLEAN NOT NULL, payload TEXT)",
            "INSERT INTO compat_partial_index_equivalence_rows VALUES \
               (1, 30, TRUE, 'delete'), (2, 20, FALSE, 'enter'), \
               (3, 30, TRUE, 'leave'), (4, 40, FALSE, 'inactive-40'), \
               (5, 30, TRUE, 'key-leave'), (6, 30, TRUE, 'stay'), \
               (7, 70, FALSE, 'inactive-70'), (8, 80, FALSE, 'inactive-80'), \
               (9, 90, FALSE, 'inactive-90'), (10, 100, FALSE, 'inactive-100'), \
               (11, 110, FALSE, 'inactive-110'), (12, 120, FALSE, 'inactive-120')",
        ],
    )
    .await
    .expect("create populated partial-index rows");
    create_active_partial_index(client).await;
}

async fn verify_partial_index_rollback(client: &tokio_postgres::Client) {
    client
        .batch_execute("BEGIN")
        .await
        .expect("begin rollback probe");
    client
        .batch_execute(
            "UPDATE compat_partial_index_equivalence_rows \
             SET active = TRUE, key_value = 30, payload = 'rolled-back' WHERE id = 2",
        )
        .await
        .expect("stage a row entering the partial predicate");
    client
        .batch_execute(
            "INSERT INTO compat_partial_index_equivalence_rows \
             VALUES (13, 30, TRUE, 'rolled-back-insert')",
        )
        .await
        .expect("stage an inserted row inside the partial predicate");
    assert_eq!(
        query_active_partial_index_rows(client).await,
        vec![
            (30, "delete".to_owned()),
            (30, "key-leave".to_owned()),
            (30, "leave".to_owned()),
            (30, "rolled-back".to_owned()),
            (30, "rolled-back-insert".to_owned()),
            (30, "stay".to_owned())
        ]
    );
    client
        .batch_execute("ROLLBACK")
        .await
        .expect("discard the staged partial-index entry");
    let plan = active_partial_index_plan(client).await;
    assert!(
        plan.contains("compat_partial_index_equivalence_idx"),
        "expected rollback to retain the original partial index, got {plan}"
    );
    assert_eq!(
        query_active_partial_index_rows(client).await,
        vec![
            (30, "delete".to_owned()),
            (30, "key-leave".to_owned()),
            (30, "leave".to_owned()),
            (30, "stay".to_owned())
        ]
    );
}

async fn commit_partial_index_writes(client: &tokio_postgres::Client) {
    client
        .batch_execute("BEGIN")
        .await
        .expect("begin committed writes");
    client
        .batch_execute(
            "INSERT INTO compat_partial_index_equivalence_rows \
             VALUES (13, 30, TRUE, 'inserted')",
        )
        .await
        .expect("add an inserted row to the partial index");
    client
        .batch_execute(
            "UPDATE compat_partial_index_equivalence_rows \
             SET active = TRUE, key_value = 30, payload = 'entered' WHERE id = 2",
        )
        .await
        .expect("add a row to the partial index");
    client
        .batch_execute(
            "UPDATE compat_partial_index_equivalence_rows \
             SET active = FALSE WHERE id = 3",
        )
        .await
        .expect("remove a row from the partial index predicate");
    client
        .batch_execute(
            "UPDATE compat_partial_index_equivalence_rows \
             SET key_value = 40, payload = 'key-updated' WHERE id = 5",
        )
        .await
        .expect("update indexed and included values");
    client
        .batch_execute(
            "UPDATE compat_partial_index_equivalence_rows \
             SET payload = 'stayed' WHERE id = 6",
        )
        .await
        .expect("update an included value while the row stays indexed");
    client
        .batch_execute("COMMIT")
        .await
        .expect("publish partial-index maintenance with row updates");
    client
        .batch_execute("DELETE FROM compat_partial_index_equivalence_rows WHERE id = 1")
        .await
        .expect("delete a row from the partial index");
}

#[test]
fn should_preserve_partial_index_query_results_for_null_excluded_rows_through_lifecycle() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("partial_index_query_equivalence").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_partial_index_equivalence_rows \
                   (id INT PRIMARY KEY, key_value INT, active BOOLEAN NOT NULL, payload TEXT)",
                "INSERT INTO compat_partial_index_equivalence_rows VALUES \
                   (1, NULL, TRUE, 'active-null'), (2, NULL, FALSE, 'excluded-null'), \
                   (3, 5, TRUE, 'active-five-a'), (4, 5, FALSE, 'excluded-five'), \
                   (5, 5, TRUE, 'active-five-b'), (6, 9, TRUE, 'active-nine')",
            ],
        )
        .await
        .expect("create active and excluded rows with both NULL and non-NULL keys");

        let active_before =
            query_partial_index_rows(&client, "active", "key_value ASC NULLS FIRST, id").await;
        let nulls_before =
            query_partial_index_rows(&client, "key_value IS NULL", "active DESC, id").await;

        // Act
        client
            .batch_execute(
                "CREATE INDEX compat_partial_index_equivalence_idx \
                 ON compat_partial_index_equivalence_rows \
                 (key_value ASC NULLS FIRST) INCLUDE (payload) WHERE active",
            )
            .await
            .expect("create a partial index that includes active NULL keys");
        let active_after_create =
            query_partial_index_rows(&client, "active", "key_value ASC NULLS FIRST, id").await;
        let nulls_after_create =
            query_partial_index_rows(&client, "key_value IS NULL", "active DESC, id").await;
        client
            .batch_execute("DROP INDEX compat_partial_index_equivalence_idx")
            .await
            .expect("remove the partial index after rebuilding it");
        let active_after_drop =
            query_partial_index_rows(&client, "active", "key_value ASC NULLS FIRST, id").await;
        let nulls_after_drop =
            query_partial_index_rows(&client, "key_value IS NULL", "active DESC, id").await;

        // Assert
        let expected_active = vec![
            (1, None, true, "active-null".to_owned()),
            (3, Some(5), true, "active-five-a".to_owned()),
            (5, Some(5), true, "active-five-b".to_owned()),
            (6, Some(9), true, "active-nine".to_owned()),
        ];
        let expected_nulls = vec![
            (1, None, true, "active-null".to_owned()),
            (2, None, false, "excluded-null".to_owned()),
        ];
        assert_eq!(active_before, expected_active);
        assert_eq!(active_after_create, expected_active);
        assert_eq!(active_after_drop, expected_active);
        assert_eq!(nulls_before, expected_nulls);
        assert_eq!(nulls_after_create, expected_nulls);
        assert_eq!(nulls_after_drop, expected_nulls);

        drop(client);
        server.shutdown(connection).await;
    });
}

async fn create_partial_index_schema_move_fixture(client: &tokio_postgres::Client) {
    execute_statements(
        client,
        &[
            "CREATE SCHEMA partial_move_source",
            "CREATE SCHEMA partial_move_target",
            "CREATE TABLE partial_move_source.rows \
               (id INT PRIMARY KEY, key_value INT, active BOOLEAN NOT NULL, payload TEXT)",
            "INSERT INTO partial_move_source.rows VALUES \
               (1, 30, TRUE, 'a'), (2, 20, FALSE, 'enter'), \
               (3, 30, TRUE, 'b'), (4, 40, FALSE, 'inactive-40'), \
               (5, 30, TRUE, 'c'), (6, 30, TRUE, 'd'), \
               (7, 70, FALSE, 'inactive-70'), (8, 80, FALSE, 'inactive-80'), \
               (9, 90, FALSE, 'inactive-90'), (10, 100, FALSE, 'inactive-100'), \
               (11, 110, FALSE, 'inactive-110'), (12, 120, FALSE, 'inactive-120')",
            "CREATE INDEX partial_move_idx ON partial_move_source.rows \
               (key_value) INCLUDE (payload) WHERE active AND key_value = 30",
            "CREATE UNIQUE INDEX partial_move_payload_uq ON partial_move_source.rows (payload)",
        ],
    )
    .await
    .expect("create a populated table and partial covering index");
}

fn expected_partial_index_schema_move_rows() -> Vec<(i32, String)> {
    vec![
        (30, "a".to_owned()),
        (30, "b".to_owned()),
        (30, "c".to_owned()),
        (30, "d".to_owned()),
    ]
}

async fn assert_partial_index_plan_uses_index(
    client: &tokio_postgres::Client,
    relation: &str,
    phase: &str,
) {
    let plan = moved_partial_index_plan(client, relation).await;
    assert!(
        plan.contains("partial_move_idx"),
        "expected the {phase} lookup to use the partial index, got {plan}"
    );
}

async fn assert_partial_move_unique_index_rejects_duplicate(
    client: &tokio_postgres::Client,
    relation: &str,
    phase: &str,
) {
    let sql = format!("INSERT INTO {relation} VALUES (13, 130, FALSE, 'a')");
    let message = format!("the {phase} unique index should reject a duplicate");
    let error = client.batch_execute(&sql).await.expect_err(&message);
    assert_eq!(db_error(&error).code().code(), "23505");
}

async fn rollback_partial_index_schema_move(
    client: &tokio_postgres::Client,
    expected: &[(i32, String)],
) {
    client
        .batch_execute("BEGIN")
        .await
        .expect("begin schema-move rollback probe");
    client
        .batch_execute("ALTER TABLE partial_move_source.rows SET SCHEMA partial_move_target")
        .await
        .expect("move the indexed table inside a transaction");
    assert_eq!(
        query_moved_partial_index_rows(client, "partial_move_target.rows").await,
        expected
    );
    client
        .batch_execute("ROLLBACK")
        .await
        .expect("roll back the indexed table schema move");
    assert_eq!(
        query_moved_partial_index_rows(client, "partial_move_source.rows").await,
        expected
    );
    assert_partial_index_plan_uses_index(client, "partial_move_source.rows", "rollback").await;
    assert_partial_move_unique_index_rejects_duplicate(
        client,
        "partial_move_source.rows",
        "source",
    )
    .await;
}

#[test]
fn should_preserve_partial_covering_index_across_table_schema_move() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("partial_index_schema_move").await;
        let (client, connection) = server.connect().await;
        create_partial_index_schema_move_fixture(&client).await;
        let expected = expected_partial_index_schema_move_rows();
        assert_eq!(
            query_moved_partial_index_rows(&client, "partial_move_source.rows").await,
            expected
        );
        assert_partial_index_plan_uses_index(&client, "partial_move_source.rows", "source").await;

        // Act
        rollback_partial_index_schema_move(&client, &expected).await;
        client
            .batch_execute("ALTER TABLE partial_move_source.rows SET SCHEMA partial_move_target")
            .await
            .expect("commit the indexed table schema move");
        assert_eq!(
            client
                .query(
                    "SELECT schemaname, tablename FROM pg_catalog.pg_indexes \
                     WHERE indexname = 'partial_move_idx'",
                    &[],
                )
                .await
                .expect("read the moved index catalog entry")
                .iter()
                .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![("partial_move_target".to_owned(), "rows".to_owned())]
        );
        assert_partial_index_plan_uses_index(&client, "partial_move_target.rows", "target").await;
        assert_eq!(
            query_moved_partial_index_rows(&client, "partial_move_target.rows").await,
            expected
        );
        assert_partial_move_unique_index_rejects_duplicate(
            &client,
            "partial_move_target.rows",
            "moved",
        )
        .await;
        client
            .batch_execute("DROP INDEX partial_move_target.partial_move_idx")
            .await
            .expect("drop the moved index to read through an independent scan");

        // Assert
        assert_eq!(
            query_moved_partial_index_rows(&client, "partial_move_target.rows").await,
            expected
        );
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_maintain_partial_index_entries_through_predicate_changing_writes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("partial_index_write_maintenance").await;
        let (client, connection) = server.connect().await;
        create_partial_index_write_fixture(&client).await;

        let initial_plan = active_partial_index_plan(&client).await;
        assert!(
            initial_plan.contains("compat_partial_index_equivalence_idx"),
            "expected the initial lookup to use its partial index, got {initial_plan}"
        );
        assert_eq!(
            query_active_partial_index_rows(&client).await,
            vec![
                (30, "delete".to_owned()),
                (30, "key-leave".to_owned()),
                (30, "leave".to_owned()),
                (30, "stay".to_owned())
            ]
        );
        let initial_indexed_rows = query_active_partial_index_rows(&client).await;
        verify_partial_index_scan_equivalence(&client, &initial_indexed_rows).await;
        create_active_partial_index(&client).await;
        let restored_plan = active_partial_index_plan(&client).await;
        assert!(
            restored_plan.contains("compat_partial_index_equivalence_idx"),
            "expected the recreated partial index to serve the lookup, got {restored_plan}"
        );

        // Act
        verify_partial_index_rollback(&client).await;
        let rolled_back_rows = query_active_partial_index_rows(&client).await;
        assert_eq!(rolled_back_rows, initial_indexed_rows);
        verify_partial_index_scan_equivalence(&client, &rolled_back_rows).await;
        create_active_partial_index(&client).await;
        let rollback_restored_plan = active_partial_index_plan(&client).await;
        assert!(
            rollback_restored_plan.contains("compat_partial_index_equivalence_idx"),
            "expected the recreated partial index to serve after rollback, got {rollback_restored_plan}"
        );
        commit_partial_index_writes(&client).await;

        // Assert
        let committed_plan = active_partial_index_plan(&client).await;
        assert!(
            committed_plan.contains("compat_partial_index_equivalence_idx"),
            "expected committed updates to retain the partial index, got {committed_plan}"
        );
        let indexed_after_writes = query_active_partial_index_rows(&client).await;
        assert_eq!(
            indexed_after_writes,
            vec![
                (30, "entered".to_owned()),
                (30, "inserted".to_owned()),
                (30, "stayed".to_owned())
            ]
        );
        verify_partial_index_scan_equivalence(&client, &indexed_after_writes).await;

        drop(client);
        server.shutdown(connection).await;
    });
}
