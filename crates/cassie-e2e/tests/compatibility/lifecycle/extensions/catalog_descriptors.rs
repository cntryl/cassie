use super::*;

#[test]
fn should_describe_selected_extension_catalog_fields() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("extension_catalog_descriptors").await;
        let (client, connection) = server.connect().await;

        // Act
        let installed = client
            .prepare("SELECT * FROM pg_catalog.pg_extension")
            .await
            .expect("extension descriptor");
        let available = client
            .prepare("SELECT * FROM pg_catalog.pg_available_extensions")
            .await
            .expect("available descriptor");
        let procedures = client
            .prepare("SELECT * FROM pg_catalog.pg_proc")
            .await
            .expect("selected procedure descriptor");

        // Assert
        let describe = |statement: &tokio_postgres::Statement| {
            statement
                .columns()
                .iter()
                .map(|column| (column.name().to_owned(), column.type_().oid()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            describe(&installed),
            vec![
                ("oid".to_owned(), 26),
                ("extname".to_owned(), 19),
                ("extowner".to_owned(), 26),
                ("extnamespace".to_owned(), 26),
                ("extrelocatable".to_owned(), 16),
                ("extversion".to_owned(), 25),
            ]
        );
        assert_eq!(
            describe(&available),
            vec![
                ("name".to_owned(), 19),
                ("default_version".to_owned(), 25),
                ("installed_version".to_owned(), 25),
            ]
        );
        assert_eq!(
            describe(&procedures),
            vec![
                ("oid".to_owned(), 26),
                ("proname".to_owned(), 25),
                ("prorettype".to_owned(), 26),
                ("pronamespace".to_owned(), 26),
            ]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_list_only_selected_available_extension_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("available_extension_rows").await;
        let (client, connection) = server.connect().await;

        // Act
        let rows = client
            .query(
                "SELECT name, default_version, installed_version \
                 FROM pg_catalog.pg_available_extensions ORDER BY name",
                &[],
            )
            .await
            .expect("selected available extension rows");

        // Assert
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, String>(0), "pgcrypto");
        assert_eq!(rows[0].get::<_, String>(1), "1.4");
        assert_eq!(rows[0].get::<_, Option<String>>(2), None);
        assert_eq!(rows[1].get::<_, String>(0), "vector");
        assert_eq!(rows[1].get::<_, String>(1), "0.8.7");
        assert_eq!(rows[1].get::<_, Option<String>>(2), None);

        drop(client);
        server.shutdown(connection).await;
    });
}
