#[test]
fn should_rollback_add_check_when_existing_rows_violate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("existing_row_check_ddl_atomicity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_existing_row_check_ddl (id INT PRIMARY KEY, amount INT)",
                "INSERT INTO compat_existing_row_check_ddl VALUES (1, -1)",
                "BEGIN",
                "SAVEPOINT before_add_check",
            ],
        )
        .await
        .expect("seed a row that violates the later CHECK");

        // Act
        let rejected_add = client
            .batch_execute(
                "ALTER TABLE compat_existing_row_check_ddl \
                 ADD CONSTRAINT compat_existing_row_check_name \
                 CHECK (amount >= 0)",
            )
            .await
            .expect_err("adding a validated CHECK must inspect existing rows");
        client
            .batch_execute("ROLLBACK TO SAVEPOINT before_add_check")
            .await
            .expect("restore the transaction after failed constraint DDL");
        let state = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_existing_row_check_name'",
                &[],
            )
            .await
            .expect("inspect catalog after failed constraint DDL");
        let rows = client
            .query(
                "SELECT id, amount FROM compat_existing_row_check_ddl ORDER BY id",
                &[],
            )
            .await
            .expect("read existing data after failed constraint DDL");
        client
            .batch_execute("COMMIT")
            .await
            .expect("commit after rolling back only the failed ALTER");

        // Assert
        assert_eq!(db_error(&rejected_add).code().code(), "23514");
        assert_eq!(state.get::<_, i64>(0), 0);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, i32>(1), -1);

        drop(client);
        server.shutdown(connection).await;
    });
}
