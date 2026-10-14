use super::*;

mod dependency_targets;
mod member_membership;
mod routine_alias_dependency;

#[test]
fn should_preserve_pgcrypto_schema_when_if_not_exists_names_another() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_schema_idempotence").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_crypto",
                "CREATE SCHEMA compat_crypto_other",
                "CREATE EXTENSION pgcrypto WITH SCHEMA compat_crypto",
                "CREATE EXTENSION IF NOT EXISTS pgcrypto \
                   WITH SCHEMA compat_crypto_other",
            ],
        )
        .await
        .expect("install pgcrypto and retain its first schema on IF NOT EXISTS");

        // Act
        let installed = client
            .query_one(
                "SELECT extension.extname, extension.extversion, namespace.nspname \
                 FROM pg_catalog.pg_extension AS extension \
                 JOIN pg_catalog.pg_namespace AS namespace \
                   ON namespace.oid = extension.extnamespace \
                 WHERE extension.extname = 'pgcrypto'",
                &[],
            )
            .await
            .expect("read installed pgcrypto identity and schema");
        let available = client
            .query_one(
                "SELECT name, default_version, installed_version \
                 FROM pg_catalog.pg_available_extensions \
                 WHERE name = 'pgcrypto'",
                &[],
            )
            .await
            .expect("read installed pgcrypto availability metadata");
        let digest: String = client
            .query_one(
                "SELECT encode(compat_crypto.digest('wave4', 'sha256'), 'hex')",
                &[],
            )
            .await
            .expect("call pgcrypto from its selected extension schema")
            .try_get(0)
            .expect("SHA-256 digest");
        let public_digest_count: i64 = client
            .query_one(
                "SELECT count(*) \
                 FROM pg_catalog.pg_proc AS routine \
                 JOIN pg_catalog.pg_namespace AS namespace \
                   ON namespace.oid = routine.pronamespace \
                 WHERE namespace.nspname = 'public' \
                   AND routine.proname = 'digest'",
                &[],
            )
            .await
            .expect("check that pgcrypto functions were not installed in public")
            .try_get(0)
            .expect("public digest overload count");

        // Assert
        assert_eq!(installed.get::<_, String>(0), "pgcrypto");
        assert_eq!(installed.get::<_, String>(1), "1.4");
        assert_eq!(installed.get::<_, String>(2), "compat_crypto");
        assert_eq!(available.get::<_, String>(0), "pgcrypto");
        assert_eq!(available.get::<_, String>(1), "1.4");
        assert_eq!(available.get::<_, String>(2), "1.4");
        assert_eq!(
            digest,
            "4559fbac12073e340b8be2bf5bdb10bbdc4bf627fdf4976901b4a3637146c634"
        );
        assert_eq!(public_digest_count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_restore_pgcrypto_catalog_after_cascade_rollback() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_cascade_rollback").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE EXTENSION pgcrypto",
                "CREATE VIEW compat_crypto_rollback_view AS \
                   SELECT encode(digest('wave2', 'sha256'), 'hex') AS value",
            ],
        )
        .await
        .expect("install pgcrypto and create a view dependent on its digest function");
        let installed_before = client
            .query_one(
                "SELECT \
                   (SELECT count(*) FROM pg_catalog.pg_extension \
                    WHERE extname = 'pgcrypto')::bigint, \
                   (SELECT count(*) FROM pg_catalog.pg_proc AS p \
                    JOIN pg_catalog.pg_namespace AS n ON n.oid = p.pronamespace \
                    WHERE n.nspname = 'public' AND p.proname = 'digest')::bigint, \
                   (SELECT count(*) FROM pg_catalog.pg_class \
                    WHERE relname = 'compat_crypto_rollback_view')::bigint, \
                   (SELECT installed_version \
                    FROM pg_catalog.pg_available_extensions \
                    WHERE name = 'pgcrypto'), \
                   (SELECT value FROM compat_crypto_rollback_view)",
                &[],
            )
            .await
            .expect("read the extension, member-function, dependent-view, and version rows");

        // Act
        client
            .batch_execute("BEGIN; DROP EXTENSION pgcrypto CASCADE")
            .await
            .expect("drop the extension and dependent view inside a transaction");
        let removed = client
            .query_one(
                "SELECT \
                   (SELECT count(*) FROM pg_catalog.pg_extension \
                    WHERE extname = 'pgcrypto')::bigint, \
                   (SELECT count(*) FROM pg_catalog.pg_proc AS p \
                    JOIN pg_catalog.pg_namespace AS n ON n.oid = p.pronamespace \
                    WHERE n.nspname = 'public' AND p.proname = 'digest')::bigint, \
                   (SELECT count(*) FROM pg_catalog.pg_class \
                    WHERE relname = 'compat_crypto_rollback_view')::bigint, \
                   (SELECT installed_version IS NULL \
                    FROM pg_catalog.pg_available_extensions \
                    WHERE name = 'pgcrypto')",
                &[],
            )
            .await
            .expect("read extension dependency visibility before rolling back");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the extension and dependent-view removal");
        let restored = client
            .query_one(
                "SELECT \
                   (SELECT count(*) FROM pg_catalog.pg_extension \
                    WHERE extname = 'pgcrypto')::bigint, \
                   (SELECT count(*) FROM pg_catalog.pg_proc AS p \
                    JOIN pg_catalog.pg_namespace AS n ON n.oid = p.pronamespace \
                    WHERE n.nspname = 'public' AND p.proname = 'digest')::bigint, \
                   (SELECT count(*) FROM pg_catalog.pg_class \
                    WHERE relname = 'compat_crypto_rollback_view')::bigint, \
                   (SELECT installed_version \
                    FROM pg_catalog.pg_available_extensions \
                    WHERE name = 'pgcrypto'), \
                   (SELECT value FROM compat_crypto_rollback_view)",
                &[],
            )
            .await
            .expect("read restored extension metadata and execute the dependent view");

        // Assert
        assert_eq!(installed_before.get::<_, i64>(0), 1);
        assert_eq!(installed_before.get::<_, i64>(1), 2);
        assert_eq!(installed_before.get::<_, i64>(2), 1);
        assert_eq!(installed_before.get::<_, String>(3), "1.4");
        assert_eq!(
            installed_before.get::<_, String>(4),
            "97a290c3a0e2398802573aa85ae800afeb09973f8930e40e6870583ef1559fa0"
        );
        assert_eq!(removed.get::<_, i64>(0), 0);
        assert_eq!(removed.get::<_, i64>(1), 0);
        assert_eq!(removed.get::<_, i64>(2), 0);
        assert!(removed.get::<_, bool>(3));
        assert_eq!(restored.get::<_, i64>(0), 1);
        assert_eq!(restored.get::<_, i64>(1), 2);
        assert_eq!(restored.get::<_, i64>(2), 1);
        assert_eq!(restored.get::<_, String>(3), "1.4");
        assert_eq!(
            restored.get::<_, String>(4),
            "97a290c3a0e2398802573aa85ae800afeb09973f8930e40e6870583ef1559fa0"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_apply_extension_dependencies_to_function_lifecycle() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_dependency_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE EXTENSION pgcrypto",
                "CREATE FUNCTION public.compat_pgcrypto_auto_dep(input text) \
                   RETURNS bytea LANGUAGE sql IMMUTABLE \
                   AS $$ SELECT public.digest(input, 'sha256') $$",
                "CREATE FUNCTION public.compat_pgcrypto_no_dep(input text) \
                   RETURNS bytea LANGUAGE sql IMMUTABLE \
                   AS $$ SELECT public.digest(input, 'sha256') $$",
                "ALTER FUNCTION public.compat_pgcrypto_auto_dep(text) \
                   DEPENDS ON EXTENSION pgcrypto",
                "ALTER FUNCTION public.compat_pgcrypto_no_dep(text) \
                   DEPENDS ON EXTENSION pgcrypto",
                "ALTER FUNCTION public.compat_pgcrypto_no_dep(text) \
                   NO DEPENDS ON EXTENSION pgcrypto",
            ],
        )
        .await
        .expect("mark one function dependent on pgcrypto and remove the other dependency");

        // Act
        let dependencies = client
            .query_one(
                "SELECT \
                   (SELECT count(*)::bigint \
                    FROM pg_catalog.pg_depend AS dependency \
                    JOIN pg_catalog.pg_extension AS extension \
                      ON extension.oid = dependency.refobjid \
                    WHERE extension.extname = 'pgcrypto' \
                      AND dependency.refclassid = 'pg_extension'::regclass \
                      AND dependency.classid = 'pg_proc'::regclass \
                      AND dependency.objid = \
                        'public.compat_pgcrypto_auto_dep(text)'::regprocedure \
                      AND dependency.deptype = 'x'), \
                   (SELECT count(*)::bigint \
                    FROM pg_catalog.pg_depend AS dependency \
                    JOIN pg_catalog.pg_extension AS extension \
                      ON extension.oid = dependency.refobjid \
                    WHERE extension.extname = 'pgcrypto' \
                      AND dependency.refclassid = 'pg_extension'::regclass \
                      AND dependency.classid = 'pg_proc'::regclass \
                      AND dependency.objid = \
                        'public.compat_pgcrypto_no_dep(text)'::regprocedure \
                      AND dependency.deptype = 'x')",
                &[],
            )
            .await
            .expect("read extension dependency rows before dropping pgcrypto");
        client
            .batch_execute("DROP EXTENSION pgcrypto RESTRICT")
            .await
            .expect("drop pgcrypto and its explicitly dependent function with RESTRICT");
        let remaining = client
            .query_one(
                "SELECT \
                   (SELECT count(*)::bigint \
                    FROM pg_catalog.pg_extension \
                    WHERE extname = 'pgcrypto'), \
                   to_regprocedure(\
                     'public.compat_pgcrypto_auto_dep(text)') IS NULL, \
                   to_regprocedure(\
                     'public.compat_pgcrypto_no_dep(text)') IS NOT NULL",
                &[],
            )
            .await
            .expect("read extension and function identity after the RESTRICT drop");

        // Assert
        assert_eq!(dependencies.get::<_, i64>(0), 1);
        assert_eq!(dependencies.get::<_, i64>(1), 0);
        assert_eq!(remaining.get::<_, i64>(0), 0);
        assert!(remaining.get::<_, bool>(1));
        assert!(remaining.get::<_, bool>(2));

        drop(client);
        server.shutdown(connection).await;
    });
}
