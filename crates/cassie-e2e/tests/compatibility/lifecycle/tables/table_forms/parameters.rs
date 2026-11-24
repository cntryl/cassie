#[test]
fn should_create_table_as_prepared_execute_with_typed_parameters() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("ctas_prepared_execute").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "PREPARE compat_ctas_prepared_source (INT, TEXT) AS \
                 SELECT $1 AS source_id, $2 AS source_label",
            )
            .await
            .expect("prepare typed CTAS source statement");

        // Act
        let created = client
            .batch_execute(
                "CREATE TABLE compat_ctas_execute_copy (copy_id, copy_label) \
                 AS EXECUTE compat_ctas_prepared_source (18, 'prepared')",
            )
            .await;
        let metadata = if created.is_ok() {
            Some(
                client
                    .query(
                        "SELECT a.attname, a.atttypid::INT, a.atttypmod, a.attnotnull \
                         FROM pg_attribute a \
                         WHERE a.attrelid = 'compat_ctas_execute_copy' \
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
                        "SELECT copy_id, copy_label FROM compat_ctas_execute_copy",
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
            "CTAS AS EXECUTE should materialize prepared parameters; SQLSTATE: {create_sqlstate:?}"
        );
        let metadata = metadata
            .expect("successful CTAS should expose result metadata")
            .expect("read prepared CTAS metadata");
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
            .expect("read rows created from prepared statement");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 18);
        assert_eq!(rows[0].get::<_, String>(1), "prepared");
    });
}
