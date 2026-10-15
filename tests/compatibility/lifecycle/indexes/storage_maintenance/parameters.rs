#[test]
fn should_roundtrip_gin_storage_parameters() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("gin_storage_parameter_roundtrip").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_gin_storage_rows (document TSVECTOR)",
                "CREATE INDEX compat_gin_storage_idx \
                   ON compat_gin_storage_rows USING gin (document) \
                   WITH (fastupdate = off, gin_pending_list_limit = 2048)",
            ],
        )
        .await
        .expect("create a GIN index with nondefault pending-list options");

        // Act
        let created: Option<String> = client
            .query_one(
                "SELECT reloptions::text FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_gin_storage_idx'::regclass",
                &[],
            )
            .await
            .expect("read initial GIN storage parameters")
            .try_get(0)
            .expect("initial GIN options");
        client
            .batch_execute(
                "ALTER INDEX compat_gin_storage_idx \
                 SET (fastupdate = on, gin_pending_list_limit = 4096)",
            )
            .await
            .expect("change both GIN storage parameters");
        let altered: Option<String> = client
            .query_one(
                "SELECT reloptions::text FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_gin_storage_idx'::regclass",
                &[],
            )
            .await
            .expect("read updated GIN storage parameters")
            .try_get(0)
            .expect("updated GIN options");
        client
            .batch_execute("ALTER INDEX compat_gin_storage_idx RESET (fastupdate)")
            .await
            .expect("restore the default GIN fast-update setting");
        let reset: Option<String> = client
            .query_one(
                "SELECT reloptions::text FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_gin_storage_idx'::regclass",
                &[],
            )
            .await
            .expect("read GIN options after reset")
            .try_get(0)
            .expect("reset GIN options");

        // Assert
        assert_eq!(
            created.as_deref(),
            Some("{fastupdate=off,gin_pending_list_limit=2048}")
        );
        assert_eq!(
            altered.as_deref(),
            Some("{fastupdate=on,gin_pending_list_limit=4096}")
        );
        assert_eq!(reset.as_deref(), Some("{gin_pending_list_limit=4096}"));

        drop(client);
        server.shutdown(connection).await;
    });
}
