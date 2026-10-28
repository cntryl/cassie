use super::*;

#[test]
fn should_roundtrip_work_alias_transaction() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("begin_commit_work_aliases").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_begin_commit_work (id INT PRIMARY KEY)")
            .await
            .expect("create the transaction alias fixture");

        // Act
        client
            .batch_execute(
                "BEGIN WORK; \
                 INSERT INTO compat_begin_commit_work VALUES (1); \
                 COMMIT WORK",
            )
            .await
            .expect("BEGIN WORK and COMMIT WORK commit the transaction");
        let rows = client
            .query("SELECT id FROM compat_begin_commit_work ORDER BY id", &[])
            .await
            .expect("read rows committed with WORK aliases");

        // Assert
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_prior_writes_after_rollback_work_to_savepoint() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("rollback_work_to_savepoint_alias").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_rollback_work_to_savepoint (id INT PRIMARY KEY)")
            .await
            .expect("create the savepoint alias fixture");

        // Act
        client
            .batch_execute(
                "BEGIN WORK; \
                 INSERT INTO compat_rollback_work_to_savepoint VALUES (1); \
                 SAVEPOINT keep_first; \
                 INSERT INTO compat_rollback_work_to_savepoint VALUES (2); \
                 ROLLBACK WORK TO SAVEPOINT keep_first; \
                 COMMIT WORK",
            )
            .await
            .expect("ROLLBACK WORK TO SAVEPOINT keeps earlier writes");
        let rows = client
            .query(
                "SELECT id FROM compat_rollback_work_to_savepoint ORDER BY id",
                &[],
            )
            .await
            .expect("read rows after rolling back to the savepoint");

        // Assert
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_rollback_work_transaction() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("rollback_work_alias").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_rollback_work (id INT PRIMARY KEY)")
            .await
            .expect("create the ROLLBACK WORK fixture");

        // Act
        client
            .batch_execute("BEGIN; INSERT INTO compat_rollback_work VALUES (1); ROLLBACK WORK")
            .await
            .expect("ROLLBACK WORK aborts the active transaction");
        let rows = client
            .query("SELECT id FROM compat_rollback_work ORDER BY id", &[])
            .await
            .expect("read rows after ROLLBACK WORK");

        // Assert
        assert!(rows.is_empty());
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_abort_transaction_with_abort_alias() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("abort_transaction_alias").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_abort_transaction (id INT PRIMARY KEY)")
            .await
            .expect("create the ABORT fixture");

        // Act
        client
            .batch_execute("BEGIN; INSERT INTO compat_abort_transaction VALUES (1); ABORT")
            .await
            .expect("ABORT rolls back the active transaction");
        let rows = client
            .query("SELECT id FROM compat_abort_transaction ORDER BY id", &[])
            .await
            .expect("read rows after ABORT");

        // Assert
        assert!(rows.is_empty());
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_commit_transaction_with_end_alias() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("end_transaction_alias").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_end_transaction (id INT PRIMARY KEY)")
            .await
            .expect("create the END fixture");

        // Act
        client
            .batch_execute("BEGIN TRANSACTION; INSERT INTO compat_end_transaction VALUES (3); END")
            .await
            .expect("END commits the active transaction");
        let rows = client
            .query("SELECT id FROM compat_end_transaction ORDER BY id", &[])
            .await
            .expect("read rows committed with END");

        // Assert
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 3);
        drop(client);
        server.shutdown(connection).await;
    });
}
