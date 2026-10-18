#[test]
fn should_truncate_a_relation_to_an_empty_result() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("truncate_relation").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_truncate_rows (id INT PRIMARY KEY)",
                "INSERT INTO compat_truncate_rows VALUES (1), (2)",
            ],
        )
        .await
        .expect("create rows to truncate");

        // Act
        client
            .batch_execute("TRUNCATE compat_truncate_rows")
            .await
            .expect("truncate relation");
        let count: i64 = client
            .query_one("SELECT count(*) FROM compat_truncate_rows", &[])
            .await
            .expect("count rows after truncation")
            .try_get(0)
            .expect("row count");

        // Assert
        assert_eq!(count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
