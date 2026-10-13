use super::*;

#[test]
fn should_allow_no_key_update_while_another_session_holds_key_share() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("row_lock_strength_compatibility").await;
        let (owner, owner_connection) = server.connect().await;
        let (worker, worker_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TABLE compat_lock_strength_rows (id INT PRIMARY KEY, value INT)",
                "INSERT INTO compat_lock_strength_rows VALUES (1, 10)",
                "BEGIN",
            ],
        )
        .await
        .expect("create a row and acquire a transaction owner");
        let owner_row: i32 = owner
            .query_one(
                "SELECT id FROM compat_lock_strength_rows WHERE id = 1 FOR KEY SHARE",
                &[],
            )
            .await
            .expect("acquire a key-share row lock")
            .try_get(0)
            .expect("owner row id");
        worker
            .batch_execute("BEGIN")
            .await
            .expect("begin the independent lock transaction");

        // Act
        let worker_row: i32 = worker
            .query_one(
                "SELECT id FROM compat_lock_strength_rows WHERE id = 1 \
                 FOR NO KEY UPDATE NOWAIT",
                &[],
            )
            .await
            .expect("NO KEY UPDATE is compatible with the held KEY SHARE lock")
            .try_get(0)
            .expect("worker row id");
        owner
            .batch_execute("COMMIT")
            .await
            .expect("commit the key-share owner");
        worker
            .batch_execute("COMMIT")
            .await
            .expect("commit the no-key-update worker");

        // Assert
        assert_eq!(owner_row, 1);
        assert_eq!(worker_row, 1);

        drop((owner, worker));
        server.shutdown(owner_connection).await;
        worker_connection.abort();
        let _ = worker_connection.await;
    });
}
