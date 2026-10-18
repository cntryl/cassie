use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexState {
    schema: String,
    table: String,
    index: String,
    file_node: i64,
    ready: bool,
    valid: bool,
    live: bool,
}

async fn index_states(client: &tokio_postgres::Client) -> Vec<IndexState> {
    client
        .query(
            "SELECT table_namespace.nspname, table_class.relname, \
                    index_class.relname, index_class.relfilenode::bigint, \
                    index_state.indisready, index_state.indisvalid, \
                    index_state.indislive \
             FROM pg_catalog.pg_index AS index_state \
             JOIN pg_catalog.pg_class AS index_class \
               ON index_class.oid = index_state.indexrelid \
             JOIN pg_catalog.pg_class AS table_class \
               ON table_class.oid = index_state.indrelid \
             JOIN pg_catalog.pg_namespace AS table_namespace \
               ON table_namespace.oid = table_class.relnamespace \
             WHERE table_namespace.nspname LIKE \
                   'compat_reindex_concurrent_%' \
             ORDER BY table_namespace.nspname, table_class.relname, \
                      index_class.relname",
            &[],
        )
        .await
        .expect("read index storage identity and lifecycle state")
        .iter()
        .map(|row| IndexState {
            schema: row.get(0),
            table: row.get(1),
            index: row.get(2),
            file_node: row.get(3),
            ready: row.get(4),
            valid: row.get(5),
            live: row.get(6),
        })
        .collect()
}

fn assert_ready_valid_live(indexes: &[IndexState]) {
    assert!(!indexes.is_empty(), "the fixture should contain indexes");
    for index in indexes {
        assert!(index.ready, "{} should be ready", index.index);
        assert!(index.valid, "{} should be valid", index.index);
        assert!(index.live, "{} should be live", index.index);
    }
}

fn assert_reindex_scope(
    before: &[IndexState],
    after: &[IndexState],
    is_selected: impl Fn(&IndexState) -> bool,
) {
    assert_eq!(before.len(), after.len());
    let mut rebuilt = 0;
    for (before, after) in before.iter().zip(after) {
        assert_eq!(
            (&before.schema, &before.table, &before.index),
            (&after.schema, &after.table, &after.index)
        );
        assert!(after.ready, "{} should be ready", after.index);
        assert!(after.valid, "{} should be valid", after.index);
        assert!(after.live, "{} should be live", after.index);
        if is_selected(before) {
            assert_ne!(
                before.file_node, after.file_node,
                "{} should receive a rebuilt storage file",
                before.index
            );
            rebuilt += 1;
        } else {
            assert_eq!(
                before.file_node, after.file_node,
                "{} is outside the requested reindex scope",
                before.index
            );
        }
    }
    assert!(rebuilt > 0, "the selected scope should contain an index");
}

async fn table_rows(client: &tokio_postgres::Client, table: &str) -> Vec<(i32, String)> {
    client
        .query(&format!("SELECT id, payload FROM {table} ORDER BY id"), &[])
        .await
        .expect("read table rows after index maintenance")
        .iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect()
}

async fn transaction_sqlstate(client: &tokio_postgres::Client, statement: &str) -> String {
    client
        .batch_execute("BEGIN")
        .await
        .expect("begin a transaction for the concurrent reindex statement");
    let error = client
        .batch_execute(statement)
        .await
        .expect_err("PostgreSQL requires concurrent reindexing in autocommit mode");
    let sqlstate = db_error(&error).code().code().to_owned();
    client
        .batch_execute("ROLLBACK")
        .await
        .expect("clear the transaction after the rejected reindex statement");
    sqlstate
}

#[test]
fn should_reindex_only_the_named_table_concurrently() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reindex_table_concurrent_scope").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_reindex_concurrent_table",
                "CREATE TABLE compat_reindex_concurrent_table.selected_rows \
                   (id INT PRIMARY KEY, payload TEXT)",
                "CREATE TABLE compat_reindex_concurrent_table.other_rows \
                   (id INT PRIMARY KEY, payload TEXT)",
                "INSERT INTO compat_reindex_concurrent_table.selected_rows \
                   VALUES (2, 'beta'), (1, 'alpha')",
                "INSERT INTO compat_reindex_concurrent_table.other_rows \
                   VALUES (4, 'delta'), (3, 'gamma')",
                "CREATE INDEX selected_payload_idx \
                   ON compat_reindex_concurrent_table.selected_rows (payload)",
                "CREATE INDEX other_payload_idx \
                   ON compat_reindex_concurrent_table.other_rows (payload)",
            ],
        )
        .await
        .expect("create two indexed tables with distinguishable contents");
        let before = index_states(&client).await;

        // Act
        client
            .batch_execute(
                "REINDEX TABLE CONCURRENTLY \
                 compat_reindex_concurrent_table.selected_rows",
            )
            .await
            .expect("rebuild the named table's indexes concurrently");
        let after = index_states(&client).await;
        let selected_rows =
            table_rows(&client, "compat_reindex_concurrent_table.selected_rows").await;
        let other_rows = table_rows(&client, "compat_reindex_concurrent_table.other_rows").await;

        // Assert
        assert_reindex_scope(&before, &after, |index| index.table == "selected_rows");
        assert_ready_valid_live(&after);
        assert_eq!(
            selected_rows,
            vec![(1, "alpha".to_owned()), (2, "beta".to_owned())]
        );
        assert_eq!(
            other_rows,
            vec![(3, "gamma".to_owned()), (4, "delta".to_owned())]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reindex_only_indexes_in_the_named_schema_concurrently() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reindex_schema_concurrent_scope").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_reindex_concurrent_selected",
                "CREATE SCHEMA compat_reindex_concurrent_outside",
                "CREATE TABLE compat_reindex_concurrent_selected.first_rows \
                   (id INT PRIMARY KEY, payload TEXT)",
                "CREATE TABLE compat_reindex_concurrent_selected.second_rows \
                   (id INT PRIMARY KEY, payload TEXT)",
                "CREATE TABLE compat_reindex_concurrent_outside.outside_rows \
                   (id INT PRIMARY KEY, payload TEXT)",
                "INSERT INTO compat_reindex_concurrent_selected.first_rows \
                   VALUES (2, 'two'), (1, 'one')",
                "INSERT INTO compat_reindex_concurrent_selected.second_rows \
                   VALUES (4, 'four'), (3, 'three')",
                "INSERT INTO compat_reindex_concurrent_outside.outside_rows \
                   VALUES (6, 'six'), (5, 'five')",
                "CREATE INDEX first_payload_idx \
                   ON compat_reindex_concurrent_selected.first_rows (payload)",
                "CREATE INDEX second_payload_idx \
                   ON compat_reindex_concurrent_selected.second_rows (payload)",
                "CREATE INDEX outside_payload_idx \
                   ON compat_reindex_concurrent_outside.outside_rows (payload)",
            ],
        )
        .await
        .expect("create indexes in the target and unrelated schemas");
        let before = index_states(&client).await;

        // Act
        client
            .batch_execute("REINDEX SCHEMA CONCURRENTLY compat_reindex_concurrent_selected")
            .await
            .expect("rebuild indexes in the named schema concurrently");
        let after = index_states(&client).await;
        let first_rows = table_rows(&client, "compat_reindex_concurrent_selected.first_rows").await;
        let second_rows =
            table_rows(&client, "compat_reindex_concurrent_selected.second_rows").await;
        let outside_rows =
            table_rows(&client, "compat_reindex_concurrent_outside.outside_rows").await;

        // Assert
        assert_reindex_scope(&before, &after, |index| {
            index.schema == "compat_reindex_concurrent_selected"
        });
        assert_ready_valid_live(&after);
        assert_eq!(
            first_rows,
            vec![(1, "one".to_owned()), (2, "two".to_owned())]
        );
        assert_eq!(
            second_rows,
            vec![(3, "three".to_owned()), (4, "four".to_owned())]
        );
        assert_eq!(
            outside_rows,
            vec![(5, "five".to_owned()), (6, "six".to_owned())]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_require_autocommit_for_concurrent_table_reindex() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reindex_table_concurrent_tx").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_reindex_concurrent_table_tx",
                "CREATE TABLE compat_reindex_concurrent_table_tx.rows \
                   (id INT PRIMARY KEY, payload TEXT)",
                "INSERT INTO compat_reindex_concurrent_table_tx.rows \
                   VALUES (2, 'two'), (1, 'one')",
                "CREATE INDEX payload_idx \
                   ON compat_reindex_concurrent_table_tx.rows (payload)",
            ],
        )
        .await
        .expect("create a table with indexes and rows for the transaction check");
        let before = index_states(&client).await;

        // Act
        let sqlstate = transaction_sqlstate(
            &client,
            "REINDEX TABLE CONCURRENTLY compat_reindex_concurrent_table_tx.rows",
        )
        .await;
        let after = index_states(&client).await;
        let rows = table_rows(&client, "compat_reindex_concurrent_table_tx.rows").await;

        // Assert
        assert_eq!(sqlstate, "25001");
        assert_eq!(
            after, before,
            "rejected maintenance must not replace indexes"
        );
        assert_ready_valid_live(&after);
        assert_eq!(rows, vec![(1, "one".to_owned()), (2, "two".to_owned())]);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_require_autocommit_for_concurrent_schema_reindex() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reindex_schema_concurrent_tx").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_reindex_concurrent_schema_tx",
                "CREATE TABLE compat_reindex_concurrent_schema_tx.rows \
                   (id INT PRIMARY KEY, payload TEXT)",
                "INSERT INTO compat_reindex_concurrent_schema_tx.rows \
                   VALUES (2, 'two'), (1, 'one')",
                "CREATE INDEX payload_idx \
                   ON compat_reindex_concurrent_schema_tx.rows (payload)",
            ],
        )
        .await
        .expect("create schema indexes and rows for the transaction check");
        let before = index_states(&client).await;

        // Act
        let sqlstate = transaction_sqlstate(
            &client,
            "REINDEX SCHEMA CONCURRENTLY compat_reindex_concurrent_schema_tx",
        )
        .await;
        let after = index_states(&client).await;
        let rows = table_rows(&client, "compat_reindex_concurrent_schema_tx.rows").await;

        // Assert
        assert_eq!(sqlstate, "25001");
        assert_eq!(
            after, before,
            "rejected maintenance must not replace indexes"
        );
        assert_ready_valid_live(&after);
        assert_eq!(rows, vec![(1, "one".to_owned()), (2, "two".to_owned())]);

        drop(client);
        server.shutdown(connection).await;
    });
}
