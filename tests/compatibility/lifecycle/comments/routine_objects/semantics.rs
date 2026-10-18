#[test]
fn should_read_then_clear_a_schema_comment() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("schema_routine_comments").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SCHEMA compat_comment_scope")
            .await
            .expect("create schema for comment lookup");
        client
            .batch_execute("COMMENT ON SCHEMA compat_comment_scope IS 'schema documentation'")
            .await
            .expect("annotate schema");

        // Act
        let comments = client
            .query_one(
                "SELECT obj_description(oid, 'pg_namespace') \
                 FROM pg_catalog.pg_namespace WHERE nspname = 'compat_comment_scope'",
                &[],
            )
            .await
            .expect("read schema comment");
        let comment = comments.try_get::<_, String>(0).expect("schema comment");
        client
            .batch_execute("COMMENT ON SCHEMA compat_comment_scope IS NULL")
            .await
            .expect("clear the schema comment");
        let schema_comment_cleared: bool = client
            .query_one(
                "SELECT obj_description(oid, 'pg_namespace') IS NULL \
                 FROM pg_catalog.pg_namespace WHERE nspname = 'compat_comment_scope'",
                &[],
            )
            .await
            .expect("read cleared schema comment")
            .try_get(0)
            .expect("schema comment nullness");

        // Assert
        assert_eq!(comment, "schema documentation");
        assert!(schema_comment_cleared);

        drop(client);
        server.shutdown(connection).await;
    });
}
