use super::*;

#[test]
fn should_allow_non_key_updates_but_block_key_updates_under_key_share() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("key_share_dml_matrix").await;
        let (owner, owner_connection) = server.connect().await;
        let (worker, worker_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TABLE compat_key_share_dml (id INT PRIMARY KEY, value INT NOT NULL)",
                "INSERT INTO compat_key_share_dml VALUES (1, 10)",
                "BEGIN",
            ],
        )
        .await
        .expect("create the row-lock fixture and begin the owner transaction");
        let owner_lock = owner
            .query_one(
                "SELECT id FROM compat_key_share_dml WHERE id = 1 FOR KEY SHARE",
                &[],
            )
            .await;

        // Act
        let observations = if owner_lock.is_ok() {
            let non_key_update = worker
                .execute(
                    "UPDATE compat_key_share_dml SET value = 20 WHERE id = 1",
                    &[],
                )
                .await;
            let contender_started = worker
                .batch_execute("SET lock_timeout = '50ms'; BEGIN")
                .await;
            let key_update = if contender_started.is_ok() {
                Some(
                    worker
                        .execute("UPDATE compat_key_share_dml SET id = 2 WHERE id = 1", &[])
                        .await,
                )
            } else {
                None
            };
            let failed_followup = if key_update.as_ref().is_some_and(|result| result.is_err()) {
                Some(worker.query_one("SELECT 1", &[]).await)
            } else {
                None
            };
            let contender_rollback = worker.batch_execute("ROLLBACK").await;
            owner
                .batch_execute("ROLLBACK")
                .await
                .expect("release the owner key-share lock");
            let timeout_reset = worker.batch_execute("RESET lock_timeout").await;
            let retry = worker
                .execute("UPDATE compat_key_share_dml SET id = 2 WHERE id = 1", &[])
                .await;
            let final_row = worker
                .query_one("SELECT id, value FROM compat_key_share_dml", &[])
                .await;
            Some((
                non_key_update,
                contender_started,
                key_update,
                failed_followup,
                contender_rollback,
                timeout_reset,
                retry,
                final_row,
            ))
        } else {
            owner
                .batch_execute("ROLLBACK")
                .await
                .expect("recover the owner after the unsupported lock clause");
            None
        };

        // Assert
        drop((owner, worker));
        server.shutdown(owner_connection).await;
        worker_connection.abort();
        let _ = worker_connection.await;

        assert!(
            owner_lock.is_ok(),
            "PostgreSQL accepts FOR KEY SHARE: {owner_lock:?}"
        );
        let (
            non_key_update,
            contender_started,
            key_update,
            failed_followup,
            contender_rollback,
            timeout_reset,
            retry,
            final_row,
        ) = observations.expect("DML lock compatibility observations");
        assert_eq!(non_key_update.expect("non-key UPDATE is compatible"), 1);
        contender_started.expect("begin the key-column update with a bounded lock wait");
        let key_update = key_update.expect("attempt the key-column update");
        assert_eq!(
            db_error(&key_update.expect_err("KEY SHARE blocks a key-column UPDATE"))
                .code()
                .code(),
            "55P03"
        );
        assert_eq!(
            db_error(
                &failed_followup
                    .expect("observe the failed transaction")
                    .expect_err("the lock timeout aborts the explicit transaction")
            )
            .code()
            .code(),
            "25P02"
        );
        contender_rollback.expect("recover after the timed-out key update");
        timeout_reset.expect("restore the session lock timeout");
        assert_eq!(retry.expect("retry after lock release"), 1);
        let final_row = final_row.expect("read the row after the retry");
        assert_eq!(
            (final_row.get::<_, i32>(0), final_row.get::<_, i32>(1)),
            (2, 20)
        );
    });
}

#[test]
fn should_block_delete_while_key_share_is_held() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("key_share_delete_conflict").await;
        let (owner, owner_connection) = server.connect().await;
        let (worker, worker_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TABLE compat_key_share_delete (id INT PRIMARY KEY)",
                "INSERT INTO compat_key_share_delete VALUES (1)",
                "BEGIN",
            ],
        )
        .await
        .expect("create the row-lock fixture and begin the owner transaction");
        let owner_lock = owner
            .query_one(
                "SELECT id FROM compat_key_share_delete WHERE id = 1 FOR KEY SHARE",
                &[],
            )
            .await;

        // Act
        let observations = if owner_lock.is_ok() {
            let contender_started = worker
                .batch_execute("SET lock_timeout = '50ms'; BEGIN")
                .await;
            let delete = if contender_started.is_ok() {
                Some(
                    worker
                        .execute("DELETE FROM compat_key_share_delete WHERE id = 1", &[])
                        .await,
                )
            } else {
                None
            };
            let failed_followup = if delete.as_ref().is_some_and(|result| result.is_err()) {
                Some(worker.query_one("SELECT 1", &[]).await)
            } else {
                None
            };
            let contender_rollback = worker.batch_execute("ROLLBACK").await;
            owner
                .batch_execute("ROLLBACK")
                .await
                .expect("release the owner key-share lock");
            let timeout_reset = worker.batch_execute("RESET lock_timeout").await;
            let retry = worker
                .execute("DELETE FROM compat_key_share_delete WHERE id = 1", &[])
                .await;
            let remaining = worker
                .query_one("SELECT count(*) FROM compat_key_share_delete", &[])
                .await;
            Some((
                contender_started,
                delete,
                failed_followup,
                contender_rollback,
                timeout_reset,
                retry,
                remaining,
            ))
        } else {
            owner
                .batch_execute("ROLLBACK")
                .await
                .expect("recover the owner after the unsupported lock clause");
            None
        };

        // Assert
        drop((owner, worker));
        server.shutdown(owner_connection).await;
        worker_connection.abort();
        let _ = worker_connection.await;

        assert!(
            owner_lock.is_ok(),
            "PostgreSQL accepts FOR KEY SHARE: {owner_lock:?}"
        );
        let (
            contender_started,
            delete,
            failed_followup,
            contender_rollback,
            timeout_reset,
            retry,
            remaining,
        ) = observations.expect("DELETE lock conflict observations");
        contender_started.expect("begin the competing delete with a bounded lock wait");
        assert_eq!(
            db_error(
                &delete
                    .expect("attempt the blocked delete")
                    .expect_err("KEY SHARE blocks DELETE until the owner releases its lock")
            )
            .code()
            .code(),
            "55P03"
        );
        assert_eq!(
            db_error(
                &failed_followup
                    .expect("observe the failed transaction")
                    .expect_err("the lock timeout aborts the explicit transaction")
            )
            .code()
            .code(),
            "25P02"
        );
        contender_rollback.expect("recover after the timed-out delete");
        timeout_reset.expect("restore the session lock timeout");
        assert_eq!(retry.expect("retry DELETE after lock release"), 1);
        assert_eq!(
            remaining
                .expect("count rows after the successful delete")
                .get::<_, i64>(0),
            0
        );
    });
}

#[test]
fn should_release_row_lock_acquired_after_a_rolled_back_savepoint() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("savepoint_row_lock_release").await;
        let (owner, owner_connection) = server.connect().await;
        let (worker, worker_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TABLE compat_savepoint_row_lock (id INT PRIMARY KEY)",
                "INSERT INTO compat_savepoint_row_lock VALUES (1)",
                "BEGIN",
                "SAVEPOINT before_row_lock",
            ],
        )
        .await
        .expect("create the row-lock fixture and savepoint");
        let owner_lock = owner
            .query_one(
                "SELECT id FROM compat_savepoint_row_lock WHERE id = 1 FOR UPDATE",
                &[],
            )
            .await;

        // Act
        let savepoint_rollback = owner
            .batch_execute("ROLLBACK TO SAVEPOINT before_row_lock")
            .await;
        let worker_begin = worker.batch_execute("BEGIN").await;
        let worker_lock = if worker_begin.is_ok() {
            Some(
                worker
                    .query_one(
                        "SELECT id FROM compat_savepoint_row_lock WHERE id = 1 FOR UPDATE NOWAIT",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };
        let worker_rollback = worker.batch_execute("ROLLBACK").await;
        let owner_rollback = owner.batch_execute("ROLLBACK").await;

        // Assert
        drop((owner, worker));
        server.shutdown(owner_connection).await;
        worker_connection.abort();
        let _ = worker_connection.await;

        assert_eq!(
            owner_lock
                .expect("acquire the row lock after the savepoint")
                .get::<_, i32>(0),
            1
        );
        savepoint_rollback.expect("ROLLBACK TO releases locks acquired after the savepoint");
        worker_begin.expect("begin the independent lock contender");
        assert_eq!(
            worker_lock
                .expect("attempt the row lock after savepoint rollback")
                .expect("the rolled-back row lock no longer blocks NOWAIT")
                .get::<_, i32>(0),
            1
        );
        worker_rollback.expect("release the contender row lock");
        owner_rollback.expect("finish the owner transaction");
    });
}
