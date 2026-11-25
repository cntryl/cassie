use super::*;

#[test]
fn should_install_only_vector_identity_without_claiming_vector_sql_semantics() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("vector_extension_identity").await;
        let (client, connection) = server.connect().await;

        // Act
        client
            .batch_execute("CREATE EXTENSION vector WITH SCHEMA public VERSION '0.8.7'")
            .await
            .expect("install the selected vector extension identity");
        client
            .batch_execute("CREATE EXTENSION IF NOT EXISTS vector VERSION '0.8.7'")
            .await
            .expect("repeat the selected vector identity installation");
        let extension = client
            .query(
                "SELECT extname, extversion FROM pg_catalog.pg_extension \
                 WHERE extname = 'vector'",
                &[],
            )
            .await
            .expect("vector extension identity");
        let available = client
            .query_one(
                "SELECT installed_version FROM pg_catalog.pg_available_extensions \
                 WHERE name = 'vector'",
                &[],
            )
            .await
            .expect("vector available-extension row");

        // Assert
        assert_eq!(extension.len(), 1);
        assert_eq!(extension[0].get::<_, String>(0), "vector");
        assert_eq!(extension[0].get::<_, String>(1), "0.8.7");
        assert_eq!(
            available.get::<_, Option<String>>(0),
            Some("0.8.7".to_owned())
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_remove_vector_extension_identity_without_touching_other_extensions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("vector_extension_drop").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE EXTENSION vector")
            .await
            .expect("install the selected vector identity");
        client
            .batch_execute("CREATE EXTENSION pgcrypto")
            .await
            .expect("install the selected pgcrypto identity");

        // Act
        client
            .batch_execute("DROP EXTENSION vector")
            .await
            .expect("remove only the vector extension identity");
        let extensions = client
            .query(
                "SELECT extname FROM pg_catalog.pg_extension ORDER BY extname",
                &[],
            )
            .await
            .expect("remaining extension identities");
        let available = client
            .query_one(
                "SELECT installed_version FROM pg_catalog.pg_available_extensions \
                 WHERE name = 'vector'",
                &[],
            )
            .await
            .expect("vector availability after drop");

        // Assert
        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0].get::<_, String>(0), "pgcrypto");
        assert_eq!(available.get::<_, Option<String>>(0), None);

        drop(client);
        server.shutdown(connection).await;
    });
}
