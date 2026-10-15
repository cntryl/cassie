use super::*;

#[test]
fn should_keep_sequence_comments_attached_to_object_identity() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("comment_sequence_identity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SEQUENCE public.compat845_sequence_identity_original",
                "COMMENT ON SEQUENCE public.compat845_sequence_identity_original \
                   IS 'sequence identity comment'",
                "ALTER SEQUENCE public.compat845_sequence_identity_original \
                   RENAME TO compat845_sequence_identity_renamed",
            ],
        )
        .await
        .expect("create, annotate, and rename the sequence");

        // Act
        let renamed = client
            .query_one(
                "SELECT obj_description(\
                         'public.compat845_sequence_identity_renamed'::regclass, 'pg_class'), \
                        (SELECT count(*) FROM pg_catalog.pg_description \
                         WHERE objoid = \
                           'public.compat845_sequence_identity_renamed'::regclass \
                           AND classoid = 'pg_catalog.pg_class'::regclass \
                           AND objsubid = 0)",
                &[],
            )
            .await
            .expect("read the renamed sequence comment and description identity");
        execute_statements(
            &client,
            &[
                "DROP SEQUENCE public.compat845_sequence_identity_renamed",
                "CREATE SEQUENCE public.compat845_sequence_identity_original",
            ],
        )
        .await
        .expect("drop the annotated sequence and reuse its former name");
        let recreated = client
            .query_one(
                "SELECT obj_description(\
                         'public.compat845_sequence_identity_original'::regclass, 'pg_class'), \
                        (SELECT count(*) FROM pg_catalog.pg_description \
                         WHERE objoid = \
                           'public.compat845_sequence_identity_original'::regclass \
                           AND classoid = 'pg_catalog.pg_class'::regclass \
                           AND objsubid = 0)",
                &[],
            )
            .await
            .expect("verify the recreated sequence has no stale comment row");

        // Assert
        assert_eq!(
            renamed.get::<_, Option<String>>(0).as_deref(),
            Some("sequence identity comment")
        );
        assert_eq!(renamed.get::<_, i64>(1), 1);
        assert_eq!(recreated.get::<_, Option<String>>(0), None);
        assert_eq!(recreated.get::<_, i64>(1), 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
