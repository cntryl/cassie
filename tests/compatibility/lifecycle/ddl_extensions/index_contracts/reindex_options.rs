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
                "SELECT indisvalid FROM pg_catalog.pg_index \
                 WHERE indexrelid = 'compat_reindex_options_idx'::regclass",
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
