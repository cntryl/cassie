#[test]
fn should_roundtrip_trigger_comments_through_trigger_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("trigger_comments").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_comment_trigger_rows (id INT)",
                "CREATE FUNCTION compat_comment_trigger_fn() RETURNS TRIGGER \
                   LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$",
                "CREATE TRIGGER compat_comment_trigger BEFORE INSERT \
                   ON compat_comment_trigger_rows FOR EACH ROW \
                   EXECUTE FUNCTION compat_comment_trigger_fn()",
            ],
        )
        .await
        .expect("create a trigger object to annotate");

        // Act
        client
            .batch_execute(
                "COMMENT ON TRIGGER compat_comment_trigger \
                   ON compat_comment_trigger_rows IS 'row insertion hook'",
            )
            .await
            .expect("store a comment on the table trigger");
        let before_clear: Option<String> = client
            .query_one(
                "SELECT obj_description(trigger.oid, 'pg_trigger') \
                 FROM pg_catalog.pg_trigger AS trigger \
                 WHERE trigger.tgname = 'compat_comment_trigger' \
                   AND trigger.tgrelid = 'compat_comment_trigger_rows'::regclass",
                &[],
            )
            .await
            .expect("read the trigger comment from pg_trigger")
            .try_get(0)
            .expect("trigger comment");
        client
            .batch_execute(
                "COMMENT ON TRIGGER compat_comment_trigger \
                   ON compat_comment_trigger_rows IS NULL",
            )
            .await
            .expect("clear the trigger comment");
        let after_clear: Option<String> = client
            .query_one(
                "SELECT obj_description(trigger.oid, 'pg_trigger') \
                 FROM pg_catalog.pg_trigger AS trigger \
                 WHERE trigger.tgname = 'compat_comment_trigger' \
                   AND trigger.tgrelid = 'compat_comment_trigger_rows'::regclass",
                &[],
            )
            .await
            .expect("read the cleared trigger comment")
            .try_get(0)
            .expect("cleared trigger comment");

        // Assert
        assert_eq!(before_clear.as_deref(), Some("row insertion hook"));
        assert_eq!(after_clear, None);

        drop(client);
        server.shutdown(connection).await;
    });
}
