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
                "SELECT name, default_version, installed_version \
                 FROM pg_catalog.pg_available_extensions \
                 WHERE name = 'pgcrypto'",
                &[],
            )
            .await
            .expect("read the available pgcrypto catalog row");

        // Assert
        assert_eq!(available.get::<_, String>(0), "pgcrypto");
        assert_eq!(available.get::<_, String>(1), "1.4");
        assert_eq!(available.get::<_, Option<String>>(2), None);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_publish_pgcrypto_installation_metadata_after_create_extension() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_installation_metadata").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE EXTENSION pgcrypto")
            .await
            .expect("install the selected crypto extension");

        // Act
        let installed = client
            .query_one(
                "SELECT e.extname, e.extversion, n.nspname \
                 FROM pg_catalog.pg_extension AS e \
                 JOIN pg_catalog.pg_namespace AS n ON n.oid = e.extnamespace \
                 WHERE e.extname = 'pgcrypto'",
                &[],
            )
            .await
            .expect("read installed pgcrypto metadata");
        let available = client
            .query_one(
                "SELECT name, default_version, installed_version \
                 FROM pg_catalog.pg_available_extensions \
                 WHERE name = 'pgcrypto'",
                &[],
            )
            .await
            .expect("read pgcrypto availability metadata after installation");

        // Assert
        assert_eq!(installed.get::<_, String>(0), "pgcrypto");
        assert_eq!(installed.get::<_, String>(1), "1.4");
        assert_eq!(installed.get::<_, String>(2), "public");
        assert_eq!(available.get::<_, String>(0), "pgcrypto");
        assert_eq!(available.get::<_, String>(1), "1.4");
        assert_eq!(available.get::<_, String>(2), "1.4");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_update_pgcrypto_from_an_available_older_version() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_version_update").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE EXTENSION pgcrypto VERSION '1.3'")
            .await
            .expect("install the available older pgcrypto version");
        let installed_before = client
            .query_one(
                "SELECT extension.extversion, available.default_version, \
                   available.installed_version \
                 FROM pg_catalog.pg_extension AS extension \
                 JOIN pg_catalog.pg_available_extensions AS available \
                   ON available.name = extension.extname \
                 WHERE extension.extname = 'pgcrypto'",
                &[],
            )
            .await
            .expect("read the installed and default pgcrypto versions before update");

        // Act
        client
            .batch_execute("ALTER EXTENSION pgcrypto UPDATE TO '1.4'")
            .await
            .expect("update pgcrypto to its PostgreSQL 18.6 default version");
        let installed_after = client
            .query_one(
                "SELECT extension.extversion, available.default_version, \
                   available.installed_version \
                 FROM pg_catalog.pg_extension AS extension \
                 JOIN pg_catalog.pg_available_extensions AS available \
                   ON available.name = extension.extname \
                 WHERE extension.extname = 'pgcrypto'",
                &[],
            )
            .await
            .expect("read the installed and default pgcrypto versions after update");
        let digest: String = client
            .query_one(
                "SELECT encode(digest('compat-extension-843', 'sha256'), 'hex')",
                &[],
            )
            .await
            .expect("call pgcrypto after updating its extension version")
            .try_get(0)
            .expect("SHA-256 digest");

        // Assert
        assert_eq!(installed_before.get::<_, String>(0), "1.3");
        assert_eq!(installed_before.get::<_, String>(1), "1.4");
        assert_eq!(
            installed_before.get::<_, Option<String>>(2).as_deref(),
            Some("1.3")
        );
        assert_eq!(installed_after.get::<_, String>(0), "1.4");
        assert_eq!(installed_after.get::<_, String>(1), "1.4");
        assert_eq!(
            installed_after.get::<_, Option<String>>(2).as_deref(),
            Some("1.4")
        );
        assert_eq!(
            digest,
            "e889342cdb1aad178a8dd34d50817242a481dfb39bbf0c009ae6291f9a0f979c"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
