#[test]
fn should_describe_whole_relation_rows_with_the_named_row_type() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("relation_row_type_descriptor_wave6").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_row_identity_wave6 (id INT, label TEXT)",
                "INSERT INTO compat_row_identity_wave6 VALUES (7, 'seven')",
            ],
        )
        .await
        .expect("create a relation with a named composite row type");

        // Act
        let row = client
            .query_one(
                "SELECT compat_row_identity_wave6 FROM compat_row_identity_wave6",
                &[],
            )
            .await
            .expect("return a relation as one whole-row value");

        // Assert
        assert_eq!(row.columns()[0].type_().name(), "compat_row_identity_wave6");
        assert_eq!(row.columns()[0].type_().schema(), "public");
        assert_ne!(row.columns()[0].type_().oid(), 2249);

        drop(client);
        server.shutdown(connection).await;
    });
}
