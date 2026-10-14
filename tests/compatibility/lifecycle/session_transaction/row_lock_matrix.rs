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

#[test]
fn should_lock_only_the_relation_named_by_for_update_of() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("row_lock_of_scope").await;
        let (owner, owner_connection) = server.connect().await;
        let (worker, worker_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TABLE compat_lock_of_left (id INT PRIMARY KEY)",
                "CREATE TABLE compat_lock_of_right (id INT PRIMARY KEY)",
                "INSERT INTO compat_lock_of_left VALUES (1)",
                "INSERT INTO compat_lock_of_right VALUES (1)",
                "BEGIN",
            ],
        )
        .await
        .expect("create the joined row-lock fixture");
        let alias_control: i32 = owner
            .query_one(
                "SELECT l.id FROM compat_lock_of_left AS l \
                 JOIN compat_lock_of_right AS r ON l.id = r.id \
                 WHERE l.id = 1",
                &[],
            )
            .await
            .expect("resolve the same aliases and predicate without a locking clause")
            .try_get(0)
            .expect("control row id");
        let owner_row: i32 = owner
            .query_one(
                "SELECT l.id FROM compat_lock_of_left AS l \
                 JOIN compat_lock_of_right AS r ON l.id = r.id \
                 WHERE l.id = 1 \
                 FOR UPDATE OF l",
                &[],
            )
            .await
            .expect("lock only the explicitly named relation")
            .try_get(0)
            .expect("owner row id");
        worker
            .batch_execute("BEGIN")
            .await
            .expect("begin the independent lock transaction");

        // Act
        let unlocked_relation_row = worker
            .query_one(
                "SELECT id FROM compat_lock_of_right WHERE id = 1 FOR UPDATE NOWAIT",
                &[],
            )
            .await
            .expect("the joined relation omitted from OF should remain unlocked");
        let selected_relation_conflict = worker
            .query_one(
                "SELECT id FROM compat_lock_of_left WHERE id = 1 FOR UPDATE NOWAIT",
                &[],
            )
            .await
            .expect_err("the relation named by OF should hold the row lock");
        owner
            .batch_execute("ROLLBACK")
            .await
            .expect("release the owner's row lock");
        worker
            .batch_execute("ROLLBACK")
            .await
            .expect("release the worker transaction after the lock conflict");

        // Assert
        assert_eq!(alias_control, 1);
        assert_eq!(owner_row, 1);
        assert_eq!(unlocked_relation_row.get::<_, i32>(0), 1);
        assert_eq!(db_error(&selected_relation_conflict).code().code(), "55P03");

        drop((owner, worker));
        server.shutdown(owner_connection).await;
        worker_connection.abort();
        let _ = worker_connection.await;
    });
}
