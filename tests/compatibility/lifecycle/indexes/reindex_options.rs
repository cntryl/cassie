use super::*;

#[test]
fn should_reindex_with_explicit_nonconcurrent_options_while_preserving_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reindex_index_options").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_reindex_options_rows (id INT PRIMARY KEY, payload TEXT)",
                "INSERT INTO compat_reindex_options_rows VALUES \
                   (3, 'gamma'), (1, 'alpha'), (2, 'beta')",
                "CREATE INDEX compat_reindex_options_idx \
                   ON compat_reindex_options_rows (payload)",
            ],
        )
        .await
        .expect("create indexed rows for the option-bearing REINDEX statement");
        let before = client
            .query(
                "SELECT id, payload FROM compat_reindex_options_rows \
                 ORDER BY payload, id",
                &[],
            )
            .await
            .expect("read rows before index maintenance");

        // Act
        client
            .batch_execute(
                "REINDEX (VERBOSE FALSE, CONCURRENTLY FALSE) \
                 INDEX compat_reindex_options_idx",
            )
            .await
            .expect("rebuild the index with explicit PostgreSQL REINDEX options");
        let after = client
            .query(
                "SELECT id, payload FROM compat_reindex_options_rows \
                 ORDER BY payload, id",
                &[],
            )
            .await
            .expect("read rows after index maintenance");
        let index_valid: bool = client
            .query_one(
                "SELECT index_state.indisvalid \
                 FROM pg_catalog.pg_index AS index_state \
                 JOIN pg_catalog.pg_class AS index_class \
                   ON index_class.oid = index_state.indexrelid_oid \
                 WHERE index_class.relname = 'compat_reindex_options_idx'",
                &[],
            )
            .await
            .expect("read rebuilt index validity")
            .try_get(0)
            .expect("index validity flag");

        // Assert
        let rows = |rows: &[tokio_postgres::Row]| {
            rows.iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>()
        };
        let expected = vec![
            (1, "alpha".to_owned()),
            (2, "beta".to_owned()),
            (3, "gamma".to_owned()),
        ];
        assert_eq!(rows(&before), expected);
        assert_eq!(rows(&after), expected);
        assert!(index_valid);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_partial_index_results_after_reindex_with_null_keys() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reindex_partial_null_keys").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_reindex_partial_rows \
                   (id INT PRIMARY KEY, key_value INT, active BOOLEAN NOT NULL)",
                "INSERT INTO compat_reindex_partial_rows VALUES \
                   (1, NULL, TRUE), (2, NULL, FALSE), (3, 5, TRUE), \
                   (4, 5, FALSE), (5, 9, TRUE)",
                "CREATE INDEX compat_reindex_partial_idx \
                   ON compat_reindex_partial_rows (key_value ASC NULLS FIRST) \
                   WHERE active",
            ],
        )
        .await
        .expect("create active and excluded rows with NULL and non-NULL keys");
        let active_before = client
            .query(
                "SELECT id, key_value FROM compat_reindex_partial_rows \
                 WHERE active ORDER BY key_value ASC NULLS FIRST, id LIMIT 10",
                &[],
            )
            .await
            .expect("read partial-index rows before rebuilding");
        let nulls_before = client
            .query(
                "SELECT id, key_value FROM compat_reindex_partial_rows \
                 WHERE key_value IS NULL ORDER BY id",
                &[],
            )
            .await
            .expect("read all NULL-key rows before rebuilding");

        // Act
        client
            .batch_execute("REINDEX INDEX compat_reindex_partial_idx")
            .await
            .expect("rebuild the partial index");
        let active_after = client
            .query(
                "SELECT id, key_value FROM compat_reindex_partial_rows \
                 WHERE active ORDER BY key_value ASC NULLS FIRST, id LIMIT 10",
                &[],
            )
            .await
            .expect("read partial-index rows after rebuilding");
        let nulls_after = client
            .query(
                "SELECT id, key_value FROM compat_reindex_partial_rows \
                 WHERE key_value IS NULL ORDER BY id",
                &[],
            )
            .await
            .expect("read all NULL-key rows after rebuilding");

        // Assert
        let rows = |rows: &[tokio_postgres::Row]| {
            rows.iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, Option<i32>>(1)))
                .collect::<Vec<_>>()
        };
        let expected_active = vec![(1, None), (3, Some(5)), (5, Some(9))];
        let expected_nulls = vec![(1, None), (2, None)];
        assert_eq!(rows(&active_before), expected_active);
        assert_eq!(rows(&active_after), expected_active);
        assert_eq!(rows(&nulls_before), expected_nulls);
        assert_eq!(rows(&nulls_after), expected_nulls);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_reindex_inside_an_explicit_transaction() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reindex_explicit_transaction").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_reindex_transaction_rows \
                   (id INT PRIMARY KEY, payload TEXT)",
                "INSERT INTO compat_reindex_transaction_rows VALUES (1, 'committed')",
                "CREATE INDEX compat_reindex_transaction_idx \
                   ON compat_reindex_transaction_rows (payload)",
            ],
        )
        .await
        .expect("create a committed row and its scalar index");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin a transaction before attempting reindex");
        client
            .batch_execute("INSERT INTO compat_reindex_transaction_rows VALUES (2, 'staged')")
            .await
            .expect("stage a row that must not leak into the durable index");

        // Act
        let error = client
            .batch_execute("REINDEX INDEX compat_reindex_transaction_idx")
            .await
            .expect_err("REINDEX is not selected for explicit transactions");
        let sqlstate = db_error(&error).code().code().to_owned();
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the transaction rejected for REINDEX");
        let visible_rows = client
            .query(
                "SELECT id, payload FROM compat_reindex_transaction_rows ORDER BY id",
                &[],
            )
            .await
            .expect("read committed table rows after rollback");

        // Assert
        assert_eq!(sqlstate, "0A000");
        assert_eq!(visible_rows.len(), 1);
        assert_eq!(visible_rows[0].get::<_, i32>(0), 1);
        assert_eq!(visible_rows[0].get::<_, String>(1), "committed");

        drop(client);
        server.shutdown(connection).await;
    });
}
