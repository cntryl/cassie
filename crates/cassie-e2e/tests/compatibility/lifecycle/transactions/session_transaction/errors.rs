#[test]
fn should_enforce_read_only_transaction_mode_with_postgres_sqlstate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("read_only_transaction_mode").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_read_only_rows (id INT PRIMARY KEY)",
                "INSERT INTO compat_read_only_rows VALUES (1)",
            ],
        )
        .await
        .expect("create the read-only transaction fixture");
        client
            .batch_execute("BEGIN TRANSACTION READ ONLY")
            .await
            .expect("begin a PostgreSQL read-only transaction");

        // Act
        let visible_rows = client
            .query("SELECT id FROM compat_read_only_rows ORDER BY id", &[])
            .await
            .expect("read-only transactions should allow SELECT");
        let rejected_write = client
            .execute("INSERT INTO compat_read_only_rows VALUES (2)", &[])
            .await
            .expect_err("read-only transactions must reject writes");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back after the rejected write");
        let stored_rows = client
            .query("SELECT id FROM compat_read_only_rows ORDER BY id", &[])
            .await
            .expect("read the committed fixture after rollback");

        // Assert
        assert_eq!(visible_rows.len(), 1);
        assert_eq!(visible_rows[0].get::<_, i32>(0), 1);
        assert_eq!(db_error(&rejected_write).code().code(), "25006");
        assert_eq!(stored_rows.len(), 1);
        assert_eq!(stored_rows[0].get::<_, i32>(0), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
