#[test]
fn should_change_table_persistence_modes_without_changing_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("table_persistence_mode_alteration").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_persistence_modes (id INT PRIMARY KEY, label TEXT)")
            .await
            .expect("create a logged table");
        client
            .execute(
                "INSERT INTO compat_persistence_modes VALUES (1, 'retained')",
                &[],
            )
            .await
            .expect("seed the table before changing its persistence mode");

        // Act
        client
            .batch_execute("ALTER TABLE compat_persistence_modes SET UNLOGGED")
            .await
            .expect("switch the table to unlogged persistence");
        let unlogged_persistence: String = client
            .query_one(
                "SELECT relpersistence::TEXT FROM pg_catalog.pg_class \
                 WHERE relname = 'compat_persistence_modes'",
                &[],
            )
            .await
            .expect("read persistence after SET UNLOGGED")
            .get(0);
        client
            .batch_execute("ALTER TABLE compat_persistence_modes SET LOGGED")
            .await
            .expect("switch the table back to logged persistence");
        let logged_persistence: String = client
            .query_one(
                "SELECT relpersistence::TEXT FROM pg_catalog.pg_class \
                 WHERE relname = 'compat_persistence_modes'",
                &[],
            )
            .await
            .expect("read persistence after SET LOGGED")
            .get(0);
        let row = client
            .query_one("SELECT id, label FROM compat_persistence_modes", &[])
            .await
            .expect("read the row after both persistence transitions");

        // Assert
        assert_eq!(unlogged_persistence, "u");
        assert_eq!(logged_persistence, "p");
        assert_eq!(row.get::<_, i32>(0), 1);
        assert_eq!(row.get::<_, String>(1), "retained");

        drop(client);
        server.shutdown(connection).await;
    });
}
