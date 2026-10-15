use super::*;

#[test]
fn should_return_a_compatible_row_with_skip_locked() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("skip_locked_compatible_row_modes").await;
        let (owner, owner_connection) = server.connect().await;
        let (worker, worker_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TABLE compat_skip_locked_compatible_modes (id INT PRIMARY KEY)",
                "INSERT INTO compat_skip_locked_compatible_modes VALUES (1)",
                "BEGIN",
            ],
        )
        .await
        .expect("create the fixture and begin the key-share owner transaction");
        let owner_row: i32 = owner
            .query_one(
                "SELECT id FROM compat_skip_locked_compatible_modes \
                 WHERE id = 1 FOR KEY SHARE",
                &[],
            )
            .await
            .expect("hold a KEY SHARE lock on the fixture row")
            .try_get(0)
            .expect("owner row id");
        worker
            .batch_execute("SET lock_timeout = '1s'; BEGIN")
            .await
            .expect("bound unexpected waits and begin the competing transaction");

        // Act
        let worker_row = worker
            .query_one(
                "SELECT id FROM compat_skip_locked_compatible_modes \
                 WHERE id = 1 FOR NO KEY UPDATE SKIP LOCKED",
                &[],
            )
            .await;
        owner
            .batch_execute("COMMIT")
            .await
            .expect("release the KEY SHARE lock after the worker query");
        let conflicting_update_lock = owner
            .query_one(
                "SELECT id FROM compat_skip_locked_compatible_modes \
                 WHERE id = 1 FOR UPDATE NOWAIT",
                &[],
            )
            .await;
        worker
            .batch_execute("ROLLBACK")
            .await
            .expect("release the worker's NO KEY UPDATE lock");
        let retry_after_worker_rollback = owner
            .query_one(
                "SELECT id FROM compat_skip_locked_compatible_modes \
                 WHERE id = 1 FOR UPDATE NOWAIT",
                &[],
            )
            .await;

        // Assert
        let worker_row = worker_row
            .map(|row| row.try_get::<_, i32>(0).expect("worker row id"))
            .map_err(|error| error.to_string());
        let conflict_code = conflicting_update_lock
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_owned());
        let retry_after_worker_rollback = retry_after_worker_rollback
            .map(|row| row.try_get::<_, i32>(0).expect("retried row id"))
            .map_err(|error| error.to_string());

        drop((owner, worker));
        server.shutdown(owner_connection).await;
        worker_connection.abort();
        let _ = worker_connection.await;

        assert_eq!(owner_row, 1);
        assert_eq!(worker_row, Ok(1), "SKIP LOCKED must keep compatible rows");
        assert_eq!(
            conflict_code.as_deref(),
            Some("55P03"),
            "the worker's NO KEY UPDATE lock must conflict with FOR UPDATE"
        );
        assert_eq!(retry_after_worker_rollback, Ok(1));
    });
}
