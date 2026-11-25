#[test]
fn should_apply_alter_sequence_increment_to_catalog_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("alter_sequence_increment").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SEQUENCE compat_increment_sequence")
            .await
            .expect("create a sequence with default options");

        // Act
        client
            .batch_execute("ALTER SEQUENCE compat_increment_sequence INCREMENT BY 2")
            .await
            .expect("change the sequence increment");
        let increment: i64 = client
            .query_one(
                "SELECT increment_by FROM pg_catalog.pg_sequences \
                 WHERE schemaname = 'public' AND sequencename = 'compat_increment_sequence'",
                &[],
            )
            .await
            .expect("read the altered sequence metadata")
            .try_get(0)
            .expect("sequence increment");

        // Assert
        assert_eq!(increment, 2);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_apply_alter_sequence_cache_to_catalog_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("alter_sequence_cache").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SEQUENCE compat_alter_cache_sequence")
            .await
            .expect("create a sequence with the default cache size");

        // Act
        client
            .batch_execute("ALTER SEQUENCE compat_alter_cache_sequence CACHE 7")
            .await
            .expect("change the sequence cache size");
        let cache_size: i64 = client
            .query_one(
                "SELECT cache_size FROM pg_catalog.pg_sequences \
                 WHERE schemaname = 'public' \
                   AND sequencename = 'compat_alter_cache_sequence'",
                &[],
            )
            .await
            .expect("read the altered sequence cache metadata")
            .try_get(0)
            .expect("sequence cache size");

        // Assert
        assert_eq!(cache_size, 7);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_publish_sequence_cache_size_in_catalog_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sequence_cache_metadata").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SEQUENCE compat_cache_sequence CACHE 3")
            .await
            .expect("create a sequence with a nondefault cache size");

        // Act
        let cache_size: i64 = client
            .query_one(
                "SELECT cache_size FROM pg_catalog.pg_sequences \
                 WHERE schemaname = 'public' \
                   AND sequencename = 'compat_cache_sequence'",
                &[],
            )
            .await
            .expect("read the configured sequence cache size")
            .try_get(0)
            .expect("sequence cache size");

        // Assert
        assert_eq!(cache_size, 3);

        drop(client);
        server.shutdown(connection).await;
    });
}
