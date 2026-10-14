use super::*;

#[test]
fn should_enable_pgcrypto_digest_after_create_extension() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_extension_identity").await;
        let (client, connection) = server.connect().await;

        // Act
        execute_statements(
            &client,
            &[
                "CREATE EXTENSION pgcrypto",
                "CREATE EXTENSION IF NOT EXISTS pgcrypto",
            ],
        )
        .await
        .expect("install the selected built-in crypto extension idempotently");
        let digest: String = client
            .query_one("SELECT encode(digest('wave4', 'sha256'), 'hex')", &[])
            .await
            .expect("call the extension function after installation")
            .try_get(0)
            .expect("SHA-256 digest");

        // Assert
        assert_eq!(
            digest,
            "4559fbac12073e340b8be2bf5bdb10bbdc4bf627fdf4976901b4a3637146c634"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_report_installed_core_extensions_with_their_schema() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_extension_catalogs").await;
        let (client, connection) = server.connect().await;

        // Act
        let installed = client
            .query(
                "SELECT e.extname, e.extversion, n.nspname \
                 FROM pg_catalog.pg_extension AS e \
                 JOIN pg_catalog.pg_namespace AS n ON n.oid = e.extnamespace \
                 WHERE e.extname = 'plpgsql'",
                &[],
            )
            .await
            .expect("read the installed core extension catalog row");

        // Assert
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].get::<_, String>(0), "plpgsql");
        assert_eq!(installed[0].get::<_, String>(1), "1.0");
        assert_eq!(installed[0].get::<_, String>(2), "pg_catalog");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_report_the_available_pgcrypto_extension_version() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_available_catalog").await;
        let (client, connection) = server.connect().await;

        // Act
        let available = client
            .query_one(
                "SELECT name, default_version \
                 FROM pg_catalog.pg_available_extensions \
                 WHERE name = 'pgcrypto'",
                &[],
            )
            .await
            .expect("read the available pgcrypto catalog row");

        // Assert
        assert_eq!(available.get::<_, String>(0), "pgcrypto");
        assert_eq!(available.get::<_, String>(1), "1.4");

        drop(client);
        server.shutdown(connection).await;
    });
}
