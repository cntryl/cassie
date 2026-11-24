
#[test]
fn should_create_table_as_values_with_result_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("ctas_values_metadata").await;
        let (client, connection) = server.connect().await;

        // Act
        let created = client
            .batch_execute(
                "CREATE TABLE compat_ctas_values_copy (copy_id, copy_label) AS \
                 VALUES (1, 'one'), (2, 'two')",
            )
            .await;
        let metadata = if created.is_ok() {
            Some(
                client
                    .query(
                        "SELECT a.attname, a.atttypid::INT, a.atttypmod, a.attnotnull \
                         FROM pg_attribute a \
                         WHERE a.attrelid = 'compat_ctas_values_copy' \
                           AND a.attnum > 0 \
                         ORDER BY attnum",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };
        let rows = if created.is_ok() {
            Some(
                client
                    .query(
                        "SELECT copy_id, copy_label FROM compat_ctas_values_copy \
                         ORDER BY copy_id",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };

        // Assert
        let create_sqlstate = created
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code());
        drop(client);
        server.shutdown(connection).await;

        assert!(
            created.is_ok(),
            "CTAS AS VALUES should materialize its rows and types; SQLSTATE: {create_sqlstate:?}"
        );
        let metadata = metadata
            .expect("successful CTAS should expose result metadata")
            .expect("read VALUES CTAS metadata");
        assert_eq!(metadata.len(), 2);
        assert_eq!(metadata[0].get::<_, String>(0), "copy_id");
        assert_eq!(metadata[0].get::<_, i32>(1), 23);
        assert_eq!(metadata[0].get::<_, i32>(2), -1);
        assert!(!metadata[0].get::<_, bool>(3));
        assert_eq!(metadata[1].get::<_, String>(0), "copy_label");
        assert_eq!(metadata[1].get::<_, i32>(1), 25);
        assert_eq!(metadata[1].get::<_, i32>(2), -1);
        assert!(!metadata[1].get::<_, bool>(3));
        let rows = rows
            .expect("successful CTAS should be queryable")
            .expect("read VALUES CTAS rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "one");
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, String>(1), "two");
    });
}
