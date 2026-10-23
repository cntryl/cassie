use super::*;

async fn extension_dependency_count(
    client: &tokio_postgres::Client,
    class_name: &str,
    object_oid: i64,
) -> i64 {
    client
        .query_one(
            "SELECT count(*)::BIGINT \
             FROM pg_catalog.pg_depend AS dependency \
             JOIN pg_catalog.pg_extension AS extension \
               ON extension.oid = dependency.refobjid \
             WHERE extension.extname = 'pgcrypto' \
               AND dependency.refclassid = 'pg_catalog.pg_extension'::regclass \
               AND dependency.classid = $1::regclass \
               AND dependency.objid = $2::oid \
               AND dependency.deptype = 'x'",
            &[&class_name, &object_oid],
        )
        .await
        .expect("read the object's explicit extension dependency")
        .get(0)
}

#[test]
fn should_reject_pgcrypto_installation_until_extension_compatibility_is_implemented() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_installation_boundary").await;
        let (client, connection) = server.connect().await;

        // Act
        let result = client.batch_execute("CREATE EXTENSION pgcrypto").await;
        let error = result.expect_err("pgcrypto compatibility remains pending under #843");

        // Assert
        assert_eq!(db_error(&error).code().code(), "0A000");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
#[ignore = "Pending #843 pgcrypto lifecycle and dependency support (v0.4)"]
fn should_track_index_dependency_on_extension() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_index_dependency").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE EXTENSION pgcrypto",
                "CREATE TABLE public.compat_extension_dependency_index_rows (id INT)",
                "CREATE INDEX compat_extension_dependency_idx \
                   ON public.compat_extension_dependency_index_rows (id)",
            ],
        )
        .await
        .expect("create pgcrypto and an independent btree index");
        let index_oid: i64 = client
            .query_one(
                "SELECT 'public.compat_extension_dependency_idx'::regclass::oid::bigint",
                &[],
            )
            .await
            .expect("resolve the index catalog identity")
            .get(0);
        let before = extension_dependency_count(&client, "pg_catalog.pg_class", index_oid).await;

        // Act
        client
            .batch_execute(
                "ALTER INDEX public.compat_extension_dependency_idx \
                 DEPENDS ON EXTENSION pgcrypto",
            )
            .await
            .expect("add an extension dependency to an index");
        let after_add = extension_dependency_count(&client, "pg_catalog.pg_class", index_oid).await;
        client
            .batch_execute(
                "ALTER INDEX public.compat_extension_dependency_idx \
                 NO DEPENDS ON EXTENSION pgcrypto",
            )
            .await
            .expect("remove the extension dependency from the index");
        let after_remove =
            extension_dependency_count(&client, "pg_catalog.pg_class", index_oid).await;

        // Assert
        assert_eq!((before, after_add, after_remove), (0, 1, 0));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
#[ignore = "Pending #843 pgcrypto lifecycle and dependency support (v0.4)"]
fn should_track_materialized_view_dependency_on_extension() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_materialized_view_dependency").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE EXTENSION pgcrypto",
                "CREATE MATERIALIZED VIEW public.compat_extension_dependency_mv \
                   AS SELECT 17 AS value",
            ],
        )
        .await
        .expect("create pgcrypto and an independent materialized view");
        let view_oid: i64 = client
            .query_one(
                "SELECT 'public.compat_extension_dependency_mv'::regclass::oid::bigint",
                &[],
            )
            .await
            .expect("resolve the materialized-view catalog identity")
            .get(0);
        let before = extension_dependency_count(&client, "pg_catalog.pg_class", view_oid).await;

        // Act
        client
            .batch_execute(
                "ALTER MATERIALIZED VIEW public.compat_extension_dependency_mv \
                 DEPENDS ON EXTENSION pgcrypto",
            )
            .await
            .expect("add an extension dependency to a materialized view");
        let after_add = extension_dependency_count(&client, "pg_catalog.pg_class", view_oid).await;
        client
            .batch_execute(
                "ALTER MATERIALIZED VIEW public.compat_extension_dependency_mv \
                 NO DEPENDS ON EXTENSION pgcrypto",
            )
            .await
            .expect("remove the extension dependency from the materialized view");
        let after_remove =
            extension_dependency_count(&client, "pg_catalog.pg_class", view_oid).await;

        // Assert
        assert_eq!((before, after_add, after_remove), (0, 1, 0));

        drop(client);
        server.shutdown(connection).await;
    });
}
