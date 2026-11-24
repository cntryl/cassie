#[test]
fn should_discard_writes_rolled_back_to_savepoint() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("savepoint_write_rollback").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_savepoint_rows (id INT PRIMARY KEY)")
            .await
            .expect("create the savepoint fixture");
        client
            .batch_execute("BEGIN; INSERT INTO compat_savepoint_rows VALUES (1)")
            .await
            .expect("begin and insert the retained row");
        client
            .batch_execute("SAVEPOINT compat_write_checkpoint")
            .await
            .expect("create a named savepoint");
        client
            .batch_execute("INSERT INTO compat_savepoint_rows VALUES (2)")
            .await
            .expect("insert a row after the savepoint");

        // Act
        client
            .batch_execute("ROLLBACK TO SAVEPOINT compat_write_checkpoint")
            .await
            .expect("roll back only work after the savepoint");
        client
            .batch_execute("RELEASE SAVEPOINT compat_write_checkpoint; COMMIT")
            .await
            .expect("release the savepoint and commit retained work");
        let ids = client
            .query("SELECT id FROM compat_savepoint_rows ORDER BY id", &[])
            .await
            .expect("read committed rows after savepoint rollback")
            .iter()
            .map(|row| row.get::<_, i32>(0))
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(ids, vec![1]);

        drop(client);
        server.shutdown(connection).await;
    });
}
