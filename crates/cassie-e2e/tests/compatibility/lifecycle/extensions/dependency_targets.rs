use super::*;

#[test]
fn should_reject_unselected_extension_names_without_catalog_publication() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("unknown_extension_boundary").await;
        let (client, connection) = server.connect().await;

        // Act
        let error = client
            .batch_execute("CREATE EXTENSION postgis")
            .await
            .expect_err("unselected extension names must be rejected");
        let installed_count = client
            .query_one("SELECT count(*) FROM pg_catalog.pg_extension", &[])
            .await
            .expect("read installed extensions after rejection")
            .get::<_, i64>(0);

        // Assert
        assert_eq!(db_error(&error).code().code(), "0A000");
        assert_eq!(installed_count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_unselected_extension_versions_without_catalog_publication() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("unsupported_extension_versions").await;
        let (client, connection) = server.connect().await;

        // Act
        let mut errors = Vec::new();
        for sql in [
            "CREATE EXTENSION pgcrypto VERSION '1.3'",
            "CREATE EXTENSION vector VERSION '0.8.6'",
        ] {
            let error = client
                .batch_execute(sql)
                .await
                .expect_err("unselected extension versions must be rejected");
            errors.push((sql, db_error(&error).code().code().to_owned()));
        }
        let installed_count = client
            .query_one("SELECT count(*) FROM pg_catalog.pg_extension", &[])
            .await
            .expect("read installed extensions after rejection")
            .get::<_, i64>(0);

        // Assert
        assert_eq!(errors.len(), 2);
        assert!(errors.iter().all(|(_, sqlstate)| sqlstate == "0A000"));
        assert_eq!(installed_count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
