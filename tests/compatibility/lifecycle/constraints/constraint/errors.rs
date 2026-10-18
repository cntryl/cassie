#[test]
fn should_reject_duplicate_null_keys_for_column_nulls_not_distinct() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("column_unique_nulls_not_distinct").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_unique_column_nulls_not_distinct (\
                   value INT CONSTRAINT compat_unique_column_nulls_not_distinct_key \
                   UNIQUE NULLS NOT DISTINCT)",
            )
            .await
            .expect("create a column-level UNIQUE NULLS NOT DISTINCT constraint");
        client
            .execute(
                "INSERT INTO compat_unique_column_nulls_not_distinct VALUES (NULL)",
                &[],
            )
            .await
            .expect("insert the first NULL key");

        // Act
        let duplicate = client
            .execute(
                "INSERT INTO compat_unique_column_nulls_not_distinct VALUES (NULL)",
                &[],
            )
            .await
            .expect_err("the column-level constraint must treat NULL keys as equal");
        let remaining = client
            .query(
                "SELECT value FROM compat_unique_column_nulls_not_distinct",
                &[],
            )
            .await
            .expect("read rows after the rejected duplicate NULL key");

        // Assert
        assert_eq!(db_error(&duplicate).code().code(), "23505");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].get::<_, Option<i32>>(0), None);

        drop(client);
        server.shutdown(connection).await;
    });
}
