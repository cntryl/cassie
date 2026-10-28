#[test]
fn should_preserve_query_results_across_index_maintenance() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("index_maintenance").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_index_rows (id INT PRIMARY KEY, score INT)",
                "INSERT INTO compat_index_rows VALUES (1, 20), (2, 10), (3, 30)",
            ],
        )
        .await
        .expect("create index table");

        // Act
        client
            .batch_execute(
                "CREATE INDEX compat_index_score_idx \
                   ON compat_index_rows (score DESC)",
            )
            .await
            .expect("create ordered index");
        client
            .batch_execute("REINDEX INDEX compat_index_score_idx")
            .await
            .expect("rebuild index");
        let index_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_indexes \
                 WHERE tablename = 'compat_index_rows' \
                   AND indexname = 'compat_index_score_idx'",
                &[],
            )
            .await
            .expect("inspect created index")
            .try_get(0)
            .expect("index count");
        let ordered = client
            .query(
                "SELECT score FROM compat_index_rows ORDER BY score DESC",
                &[],
            )
            .await
            .expect("read table after index maintenance");
        client
            .batch_execute("DROP INDEX compat_index_score_idx")
            .await
            .expect("drop an index without a table-specific extension");
        let after_drop: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_indexes \
                 WHERE tablename = 'compat_index_rows' \
                   AND indexname = 'compat_index_score_idx'",
                &[],
            )
            .await
            .expect("inspect dropped index state")
            .try_get(0)
            .expect("remaining index count");

        // Assert
        assert_eq!(index_count, 1);
        assert_eq!(after_drop, 0);
        assert_eq!(
            ordered
                .iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![30, 20, 10]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
