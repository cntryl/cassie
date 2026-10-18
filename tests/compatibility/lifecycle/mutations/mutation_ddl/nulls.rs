#[test]
fn should_enforce_unique_nulls_not_distinct_index_semantics() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("unique_index_nulls_not_distinct").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_nulls_not_distinct (value INT)",
                "CREATE UNIQUE INDEX compat_nulls_not_distinct_idx \
                   ON compat_nulls_not_distinct (value) NULLS NOT DISTINCT",
                "INSERT INTO compat_nulls_not_distinct VALUES (NULL)",
            ],
        )
        .await
        .expect("create the unique index and insert its first NULL key");

        // Act
        let duplicate_null = client
            .execute("INSERT INTO compat_nulls_not_distinct VALUES (NULL)", &[])
            .await
            .expect_err("the index must treat NULL keys as equal");
        let remaining_rows: i64 = client
            .query_one("SELECT count(*) FROM compat_nulls_not_distinct", &[])
            .await
            .expect("count rows after duplicate NULL rejection")
            .try_get(0)
            .expect("row count");

        // Assert
        assert_eq!(db_error(&duplicate_null).code().code(), "23505");
        assert_eq!(remaining_rows, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
