use super::*;

#[test]
fn should_relocate_pgcrypto_schema_with_its_member_functions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_schema_relocation").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_crypto_before_move",
                "CREATE SCHEMA compat_crypto_after_move",
                "CREATE EXTENSION pgcrypto WITH SCHEMA compat_crypto_before_move",
            ],
        )
        .await
        .expect("create the destination schemas and install pgcrypto in its starting schema");

        // Act
        client
            .batch_execute("ALTER EXTENSION pgcrypto SET SCHEMA compat_crypto_after_move")
            .await
            .expect("relocate the installed extension and its member functions");
        let extension = client
            .query_one(
                "SELECT e.extversion, n.nspname \
                 FROM pg_catalog.pg_extension AS e \
                 JOIN pg_catalog.pg_namespace AS n ON n.oid = e.extnamespace \
                 WHERE e.extname = 'pgcrypto'",
                &[],
            )
            .await
            .expect("read the relocated extension namespace");
        let digest_schemas = client
            .query_one(
                "SELECT \
                   count(*) FILTER (WHERE n.nspname = 'compat_crypto_before_move')::bigint, \
                   count(*) FILTER (WHERE n.nspname = 'compat_crypto_after_move')::bigint \
                 FROM pg_catalog.pg_proc AS p \
                 JOIN pg_catalog.pg_namespace AS n ON n.oid = p.pronamespace \
                 WHERE p.proname = 'digest' \
                   AND n.nspname IN \
                     ('compat_crypto_before_move', 'compat_crypto_after_move')",
                &[],
            )
            .await
            .expect("read digest overloads from the old and new schemas");
        let digest: String = client
            .query_one(
                "SELECT encode(compat_crypto_after_move.digest('wave7', 'sha256'), 'hex')",
                &[],
            )
            .await
            .expect("call pgcrypto from its relocated schema")
            .try_get(0)
            .expect("SHA-256 digest");
        let old_schema_error = client
            .query_one(
                "SELECT compat_crypto_before_move.digest('wave7', 'sha256')",
                &[],
            )
            .await
            .expect_err("the old schema should no longer resolve the relocated function");
        let old_schema_sqlstate = db_error(&old_schema_error).code().code();

        // Assert
        assert_eq!(extension.get::<_, String>(0), "1.4");
        assert_eq!(extension.get::<_, String>(1), "compat_crypto_after_move");
        assert_eq!(digest_schemas.get::<_, i64>(0), 0);
        assert_eq!(digest_schemas.get::<_, i64>(1), 2);
        assert_eq!(
            digest,
            "7f571684ecc28bdfed2b6552c4b470a0d89b9dbe6b9db246cef5d732e9a565d2"
        );
        assert_eq!(old_schema_sqlstate, "42883");

        drop(client);
        server.shutdown(connection).await;
    });
}
