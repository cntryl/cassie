#[test]
fn should_clear_generic_routine_comment_by_identity_signature() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("generic_routine_comment_null_clear").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE FUNCTION public.compat845_routine_comment_clear(input INT) \
                   RETURNS INT LANGUAGE SQL IMMUTABLE AS $$ SELECT input $$",
                "COMMENT ON ROUTINE public.compat845_routine_comment_clear(INT) \
                   IS 'routine note'",
            ],
        )
        .await
        .expect("create and annotate a routine with an explicit identity signature");

        // Act
        let before_clear = client
            .query_one(
                "SELECT description.classoid::regclass::TEXT, \
                        description.objsubid::SMALLINT, description.description \
                 FROM pg_catalog.pg_description AS description \
                 WHERE description.objoid = \
                   'public.compat845_routine_comment_clear(integer)'::regprocedure \
                   AND description.classoid = 'pg_catalog.pg_proc'::regclass",
                &[],
            )
            .await
            .expect("read the routine description using its identity signature");
        client
            .batch_execute(
                "COMMENT ON ROUTINE \
                 public.compat845_routine_comment_clear(integer) IS NULL",
            )
            .await
            .expect("clear the generic routine description");
        let after_clear = client
            .query_one(
                "SELECT \
                   obj_description(\
                     'public.compat845_routine_comment_clear(integer)'::regprocedure, \
                     'pg_proc') IS NULL, \
                   count(*)::BIGINT \
                 FROM pg_catalog.pg_description \
                 WHERE objoid = \
                   'public.compat845_routine_comment_clear(integer)'::regprocedure \
                   AND classoid = 'pg_catalog.pg_proc'::regclass \
                   AND objsubid = 0",
                &[],
            )
            .await
            .expect("read the cleared routine description from pg_proc");

        // Assert
        assert_eq!(
            before_clear
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![25, 21, 25]
        );
        assert_eq!(before_clear.get::<_, String>(0), "pg_proc");
        assert_eq!(before_clear.get::<_, i16>(1), 0);
        assert_eq!(before_clear.get::<_, String>(2), "routine note");
        assert!(after_clear.get::<_, bool>(0));
        assert_eq!(after_clear.get::<_, i64>(1), 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
