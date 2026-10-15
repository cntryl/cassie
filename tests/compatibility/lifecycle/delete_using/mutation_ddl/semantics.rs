#[test]
fn should_delete_rows_using_a_joined_source() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("delete_using_join").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_delete_target (id INT PRIMARY KEY, label TEXT)",
                "CREATE TABLE compat_delete_source (id INT PRIMARY KEY)",
                "INSERT INTO compat_delete_target VALUES (1, 'keep'), (2, 'delete')",
                "INSERT INTO compat_delete_source VALUES (2)",
            ],
        )
        .await
        .expect("create DELETE USING fixture rows");

        // Act
        let deleted = client
            .query(
                "DELETE FROM compat_delete_target AS target \
                 USING compat_delete_source AS source \
                 WHERE target.id = source.id RETURNING target.id",
                &[],
            )
            .await
            .expect("delete target rows matched by source");
        let remaining = client
            .query("SELECT id FROM compat_delete_target ORDER BY id", &[])
            .await
            .expect("read rows after DELETE USING");

        // Assert
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].get::<_, i32>(0), 2);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].get::<_, i32>(0), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
