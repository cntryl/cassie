#[test]
fn should_atomically_rollback_table_creation_after_insert() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("transactional_table_creation").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin the DDL transaction");

        // Act
        client
            .batch_execute("CREATE TABLE compat_transactional_ddl_rows (id INT PRIMARY KEY)")
            .await
            .expect("create a table inside the open transaction");
        client
            .batch_execute("INSERT INTO compat_transactional_ddl_rows VALUES (1)")
            .await
            .expect("insert data into the uncommitted table");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back both catalog and row changes");
        let table_count: i64 = client
            .query_one(
                "SELECT count(*) FROM information_schema.tables \
                 WHERE table_schema = 'public' \
                   AND table_name = 'compat_transactional_ddl_rows'",
                &[],
            )
            .await
            .expect("inspect table publication after rollback")
            .try_get(0)
            .expect("matching table count");

        // Assert
        assert_eq!(table_count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_rollback_alter_table_column_addition() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("transactional_alter_rollback").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_alter_rollback_rows (id INT PRIMARY KEY)",
                "INSERT INTO compat_alter_rollback_rows VALUES (1)",
                "BEGIN",
            ],
        )
        .await
        .expect("create the existing table and start a transaction");

        // Act
        client
            .batch_execute(
                "ALTER TABLE compat_alter_rollback_rows \
                 ADD COLUMN label TEXT DEFAULT 'temporary'",
            )
            .await
            .expect("add a column inside the open transaction");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the catalog mutation");
        let column_count: i64 = client
            .query_one(
                "SELECT count(*) FROM information_schema.columns \
                 WHERE table_schema = 'public' \
                   AND table_name = 'compat_alter_rollback_rows' \
                   AND column_name = 'label'",
                &[],
            )
            .await
            .expect("inspect the table after rollback")
            .try_get(0)
            .expect("matching column count");
        let rows: i64 = client
            .query_one("SELECT count(*) FROM compat_alter_rollback_rows", &[])
            .await
            .expect("read the existing row after rollback")
            .try_get(0)
            .expect("row count");

        // Assert
        assert_eq!(column_count, 0);
        assert_eq!(rows, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
