use super::*;

#[test]
fn should_describe_empty_extension_catalogs_without_advertising_unimplemented_extensions() {
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
        let installed_rows = client.query(&installed, &[]).await.expect("installed rows");
        let available_rows = client.query(&available, &[]).await.expect("available rows");

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
                ("extconfig".to_owned(), 1028),
                ("extcondition".to_owned(), 1009),
            ]
        );
        assert_eq!(
            describe(&available),
            vec![
                ("name".to_owned(), 19),
                ("default_version".to_owned(), 25),
                ("installed_version".to_owned(), 25),
                ("comment".to_owned(), 25),
            ]
        );
        assert!(installed_rows.is_empty());
        assert!(available_rows.is_empty());
        drop(client);
        server.shutdown(connection).await;
    });
}
