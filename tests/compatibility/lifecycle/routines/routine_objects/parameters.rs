#[test]
fn should_execute_parameterized_dynamic_sql_in_a_do_block() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("do_dynamic_sql").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_do_rows (id INT PRIMARY KEY)")
            .await
            .expect("create DO target");

        // Act
        client
            .batch_execute(
                "DO $block$ \
                 DECLARE target_id INT := 7; \
                 BEGIN \
                   EXECUTE 'INSERT INTO compat_do_rows VALUES ($1)' USING target_id; \
                 END; \
                 $block$",
            )
            .await
            .expect("execute parameterized dynamic SQL");
        let ids = client
            .query("SELECT id FROM compat_do_rows ORDER BY id", &[])
            .await
            .expect("read dynamic SQL result");

        // Assert
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0].get::<_, i32>(0), 7);

        drop(client);
        server.shutdown(connection).await;
    });
}
