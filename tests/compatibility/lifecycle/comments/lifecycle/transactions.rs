#[test]
fn should_restore_table_comment_at_each_rollback_scope() {
    // Arrange
    let runtime = lifecycle_wave_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("comment_transaction_rollback").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_wave2_comment_rows (id INT); \
                 COMMENT ON TABLE compat_wave2_comment_rows IS 'baseline'",
            )
            .await
            .expect("create a table with a baseline comment");

        // Act
        client
            .batch_execute(
                "BEGIN; \
                 COMMENT ON TABLE compat_wave2_comment_rows IS 'outer'; \
                 SAVEPOINT comment_inner; \
                 COMMENT ON TABLE compat_wave2_comment_rows IS 'inner'; \
                 ROLLBACK TO SAVEPOINT comment_inner",
            )
            .await
            .expect("roll back the inner comment update to its savepoint");
        let after_savepoint: Option<String> = client
            .query_one(
                "SELECT obj_description(\
                   'compat_wave2_comment_rows'::regclass, 'pg_class')",
                &[],
            )
            .await
            .expect("read the comment after savepoint rollback")
            .try_get(0)
            .expect("comment after savepoint rollback");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the outer comment update");
        let after_transaction: Option<String> = client
            .query_one(
                "SELECT obj_description(\
                   'compat_wave2_comment_rows'::regclass, 'pg_class')",
                &[],
            )
            .await
            .expect("read the comment after transaction rollback")
            .try_get(0)
            .expect("comment after transaction rollback");

        // Assert
        assert_eq!(after_savepoint.as_deref(), Some("outer"));
        assert_eq!(after_transaction.as_deref(), Some("baseline"));

        drop(client);
        server.shutdown(connection).await;
    });
}
