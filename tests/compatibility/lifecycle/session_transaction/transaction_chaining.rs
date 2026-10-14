use super::*;

#[test]
fn should_open_next_transaction_after_rollback_chain() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("rollback_and_chain").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_rollback_chain (id INT PRIMARY KEY)")
            .await
            .expect("create the rollback-chain fixture");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin the transaction to roll back");
        client
            .batch_execute("INSERT INTO compat_rollback_chain VALUES (1)")
            .await
            .expect("write in the original transaction");

        // Act
        client
            .batch_execute("ROLLBACK AND CHAIN")
            .await
            .expect("roll back and start a follow-up transaction");
        client
            .batch_execute("INSERT INTO compat_rollback_chain VALUES (2)")
            .await
            .expect("write in the chained transaction");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back only the chained transaction");
        let rows = client
            .query("SELECT id FROM compat_rollback_chain ORDER BY id", &[])
            .await
            .expect("read rows after both rollbacks");

        // Assert
        assert!(rows.is_empty());
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_commit_without_opening_next_transaction() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("commit_and_no_chain").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_commit_no_chain (id INT PRIMARY KEY)")
            .await
            .expect("create the no-chain fixture");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin the transaction to commit");
        client
            .batch_execute("INSERT INTO compat_commit_no_chain VALUES (1)")
            .await
            .expect("write in the original transaction");

        // Act
        client
            .batch_execute("COMMIT AND NO CHAIN")
            .await
            .expect("commit without starting a follow-up transaction");
        client
            .batch_execute("INSERT INTO compat_commit_no_chain VALUES (2)")
            .await
            .expect("write with autocommit after COMMIT AND NO CHAIN");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("leave an idle session unchanged after rollback");
        let rows = client
            .query("SELECT id FROM compat_commit_no_chain ORDER BY id", &[])
            .await
            .expect("read rows after the no-chain commit");

        // Assert
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        drop(client);
        server.shutdown(connection).await;
    });
}
