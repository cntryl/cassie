use super::*;

#[test]
fn should_reject_materialized_view_comments_until_metadata_support_is_implemented() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_comments").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE MATERIALIZED VIEW compat_comment_mv AS SELECT 1 AS id")
            .await
            .expect("create a materialized view to test comment rejection");

        // Act
        let result = client
            .batch_execute("COMMENT ON MATERIALIZED VIEW compat_comment_mv IS 'cached rows'")
            .await;
        let error = result.expect_err("materialized-view comments remain pending under #845");

        // Assert
        assert_eq!(db_error(&error).code().code(), "0A000");

        drop(client);
        server.shutdown(connection).await;
    });
}
