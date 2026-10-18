use super::*;

#[test]
fn should_restore_owned_sequence_when_truncate_restart_is_rolled_back() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("truncate_restart_identity_rollback").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SEQUENCE compat_truncate_identity_seq")
            .await
            .expect("create the restartable sequence");
        client
            .batch_execute(
                "CREATE TABLE compat_truncate_identity_rows (\
                   id BIGINT DEFAULT nextval('compat_truncate_identity_seq')\
                 )",
            )
            .await
            .expect("create a table with a sequence-backed default");
        client
            .batch_execute(
                "ALTER SEQUENCE compat_truncate_identity_seq \
                 OWNED BY compat_truncate_identity_rows.id",
            )
            .await
            .expect("make the sequence owned by its table column");
        let first: i64 = client
            .query_one(
                "INSERT INTO compat_truncate_identity_rows DEFAULT VALUES RETURNING id",
                &[],
            )
            .await
            .expect("insert the first sequence value")
            .try_get(0)
            .expect("sequence value");
        let second: i64 = client
            .query_one(
                "INSERT INTO compat_truncate_identity_rows DEFAULT VALUES RETURNING id",
                &[],
            )
            .await
            .expect("insert the second sequence value")
            .try_get(0)
            .expect("sequence value");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin the restart transaction");
        client
            .batch_execute("TRUNCATE TABLE compat_truncate_identity_rows RESTART IDENTITY")
            .await
            .expect("truncate and transactionally restart the owned sequence");
        let restarted: i64 = client
            .query_one(
                "INSERT INTO compat_truncate_identity_rows DEFAULT VALUES RETURNING id",
                &[],
            )
            .await
            .expect("insert after restarting the owned sequence")
            .try_get(0)
            .expect("sequence value");

        // Act
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the table truncation and sequence restart");
        let after_rollback: i64 = client
            .query_one(
                "INSERT INTO compat_truncate_identity_rows DEFAULT VALUES RETURNING id",
                &[],
            )
            .await
            .expect("insert after rolling back the restart")
            .try_get(0)
            .expect("sequence value");

        // Assert
        assert_eq!((first, second, restarted, after_rollback), (1, 2, 1, 3));

        drop(client);
        server.shutdown(connection).await;
    });
}
