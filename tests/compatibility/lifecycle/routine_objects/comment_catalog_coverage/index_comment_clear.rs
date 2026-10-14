use super::*;

#[test]
fn should_clear_index_comment_from_relation_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("index_comment_null_clear").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE public.compat845_index_comment_clear_rows (id INT)",
                "CREATE INDEX public.compat845_index_comment_clear_idx \
                   ON public.compat845_index_comment_clear_rows (id)",
                "COMMENT ON INDEX public.compat845_index_comment_clear_idx \
                   IS 'index note'",
            ],
        )
        .await
        .expect("create an index and attach its initial description");

        // Act
        let before_clear = client
            .query_one(
                "SELECT description, objsubid::SMALLINT \
                 FROM pg_catalog.pg_description \
                 WHERE objoid = 'public.compat845_index_comment_clear_idx'::regclass \
                   AND classoid = 'pg_catalog.pg_class'::regclass \
                   AND objsubid = 0",
                &[],
            )
            .await
            .expect("read the index description identity");
        client
            .batch_execute("COMMENT ON INDEX public.compat845_index_comment_clear_idx IS NULL")
            .await
            .expect("clear the index description");
        let after_clear = client
            .query_one(
                "SELECT \
                   obj_description(\
                     'public.compat845_index_comment_clear_idx'::regclass, 'pg_class') IS NULL, \
                   count(*)::BIGINT \
                 FROM pg_catalog.pg_description \
                 WHERE objoid = 'public.compat845_index_comment_clear_idx'::regclass \
                   AND classoid = 'pg_catalog.pg_class'::regclass \
                   AND objsubid = 0",
                &[],
            )
            .await
            .expect("read the cleared description through the index's catalog identity");

        // Assert
        assert_eq!(
            before_clear
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![25, 21]
        );
        assert_eq!(before_clear.get::<_, String>(0), "index note");
        assert_eq!(before_clear.get::<_, i16>(1), 0);
        assert!(after_clear.get::<_, bool>(0));
        assert_eq!(after_clear.get::<_, i64>(1), 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
