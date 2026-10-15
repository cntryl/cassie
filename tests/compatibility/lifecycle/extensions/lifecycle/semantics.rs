#[test]
fn should_preserve_extension_dependencies_until_cascade_drop() {
    // Arrange
    let runtime = lifecycle_wave_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("extension_dependency_drop").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE EXTENSION pgcrypto; \
                 CREATE VIEW compat_wave2_crypto_view AS \
                   SELECT digest('wave2', 'sha256') AS digest",
            )
            .await
            .expect("install pgcrypto and create a dependent view");

        // Act
        let restricted_drop = client
            .batch_execute("DROP EXTENSION pgcrypto")
            .await
            .expect_err("RESTRICT must reject dropping an extension with dependents");
        let retained = client
            .query_one(
                "SELECT \
                   (SELECT count(*) FROM pg_catalog.pg_extension \
                    WHERE extname = 'pgcrypto'), \
                   (SELECT count(*) FROM pg_catalog.pg_class \
                    WHERE relname = 'compat_wave2_crypto_view')",
                &[],
            )
            .await
            .expect("verify RESTRICT left the extension and view intact");
        client
            .batch_execute("DROP EXTENSION pgcrypto CASCADE")
            .await
            .expect("CASCADE removes the extension and its dependent view");
        let removed = client
            .query_one(
                "SELECT \
                   (SELECT count(*) FROM pg_catalog.pg_extension \
                    WHERE extname = 'pgcrypto'), \
                   (SELECT count(*) FROM pg_catalog.pg_class \
                    WHERE relname = 'compat_wave2_crypto_view')",
                &[],
            )
            .await
            .expect("verify the cascade removed both catalog entries");

        // Assert
        assert_eq!(db_error(&restricted_drop).code().code(), "2BP01");
        assert_eq!(retained.get::<_, i64>(0), 1);
        assert_eq!(retained.get::<_, i64>(1), 1);
        assert_eq!(removed.get::<_, i64>(0), 0);
        assert_eq!(removed.get::<_, i64>(1), 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
