use super::*;

async fn routine_extension_dependency_count(
    client: &tokio_postgres::Client,
    routine_oid: i64,
) -> i64 {
    client
        .query_one(
            "SELECT count(*)::BIGINT \
             FROM pg_catalog.pg_depend AS dependency \
             JOIN pg_catalog.pg_extension AS extension \
               ON extension.oid = dependency.refobjid \
             WHERE extension.extname = 'plpgsql' \
               AND dependency.refclassid = 'pg_catalog.pg_extension'::regclass \
               AND dependency.classid = 'pg_catalog.pg_proc'::regclass \
               AND dependency.objid = $1::oid \
               AND dependency.deptype = 'x'",
            &[&routine_oid],
        )
        .await
        .expect("read the routine's extension dependency identity")
        .get(0)
}

#[test]
fn should_track_extension_dependency_through_routine_alias() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("extension_routine_alias_dependency").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE FUNCTION public.compat843_routine_dependency(input integer) \
                 RETURNS integer LANGUAGE SQL IMMUTABLE AS $$ SELECT input + 1 $$",
            )
            .await
            .expect("create an independent SQL function with a stable routine identity");
        let routine_oid: i64 = client
            .query_one(
                "SELECT 'public.compat843_routine_dependency(integer)'::regprocedure::oid::bigint",
                &[],
            )
            .await
            .expect("resolve the SQL function's PostgreSQL routine identity")
            .get(0);
        let before = routine_extension_dependency_count(&client, routine_oid).await;

        // Act
        client
            .batch_execute(
                "ALTER ROUTINE public.compat843_routine_dependency(integer) \
                 DEPENDS ON EXTENSION plpgsql",
            )
            .await
            .expect("add an extension dependency through ALTER ROUTINE");
        let after_add = routine_extension_dependency_count(&client, routine_oid).await;
        client
            .batch_execute(
                "ALTER ROUTINE public.compat843_routine_dependency(integer) \
                 NO DEPENDS ON EXTENSION plpgsql",
            )
            .await
            .expect("remove the extension dependency through ALTER ROUTINE");
        let after_remove = routine_extension_dependency_count(&client, routine_oid).await;
        let function_result: i32 = client
            .query_one("SELECT public.compat843_routine_dependency(41)", &[])
            .await
            .expect("keep the routine callable after removing its extension dependency")
            .get(0);

        // Assert
        assert_eq!((before, after_add, after_remove), (0, 1, 0));
        assert_eq!(function_result, 42);

        drop(client);
        server.shutdown(connection).await;
    });
}
