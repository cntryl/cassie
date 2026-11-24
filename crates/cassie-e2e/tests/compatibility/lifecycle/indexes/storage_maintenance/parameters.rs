#[test]
fn should_reject_gin_storage_parameters_until_the_method_is_supported() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("gin_storage_parameter_roundtrip").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_gin_storage_rows (value INT)")
            .await
            .expect("create a table for the unsupported GIN method");

        // Act
        let rejected = client
            .batch_execute(
                "CREATE INDEX compat_gin_storage_idx \
                   ON compat_gin_storage_rows USING gin (value) \
                   WITH (fastupdate = off, gin_pending_list_limit = 2048)",
            )
            .await
            .expect_err("GIN storage parameters must not imply support for the GIN method");
        let index_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_indexes \
                 WHERE indexname = 'compat_gin_storage_idx'",
                &[],
            )
            .await
            .expect("check that the rejected GIN index was not published")
            .try_get(0)
            .expect("index count");

        // Assert
        assert_eq!(db_error(&rejected).code().code(), "0A000");
        assert_eq!(index_count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
