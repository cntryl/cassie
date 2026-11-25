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

#[test]
fn should_preserve_read_only_characteristics_given_commit_chain_when_followup_transaction_writes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("chain_read_only_characteristics").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_chain_read_only (id INT PRIMARY KEY)")
            .await
            .expect("create ordinary table");
        client
            .batch_execute("BEGIN ISOLATION LEVEL READ UNCOMMITTED, READ ONLY, DEFERRABLE")
            .await
            .expect("begin with explicit characteristics");
        client
            .query_one("SELECT 1", &[])
            .await
            .expect("read within original transaction");

        // Act
        client
            .batch_execute("COMMIT AND CHAIN")
            .await
            .expect("replace transaction owner");
        let isolation: String = client
            .query_one("SHOW transaction_isolation", &[])
            .await
            .expect("chained isolation")
            .get(0);
        let read_only: String = client
            .query_one("SHOW transaction_read_only", &[])
            .await
            .expect("chained read-only mode")
            .get(0);
        let deferrable: String = client
            .query_one("SHOW transaction_deferrable", &[])
            .await
            .expect("chained deferrable flag")
            .get(0);
        client
            .batch_execute("SET application_name = 'allowed-in-read-only'")
            .await
            .expect("session settings remain writable");
        let rejected = client
            .execute("INSERT INTO compat_chain_read_only VALUES (1)", &[])
            .await
            .expect_err("ordinary table write must fail");
        let aborted = client
            .query_one("SELECT 1", &[])
            .await
            .expect_err("failed transaction requires recovery");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("retire chained transaction");
        client
            .execute("INSERT INTO compat_chain_read_only VALUES (2)", &[])
            .await
            .expect("ordinary default is writable again");
        let stored: i64 = client
            .query_one("SELECT count(*) FROM compat_chain_read_only", &[])
            .await
            .expect("read committed rows")
            .get(0);

        // Assert
        assert_eq!(isolation, "read uncommitted");
        assert_eq!((read_only.as_str(), deferrable.as_str()), ("on", "on"));
        assert_eq!(db_error(&rejected).code().code(), "25006");
        assert_eq!(db_error(&aborted).code().code(), "25P02");
        assert_eq!(stored, 1);
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_idle_chaining_given_no_transaction_when_commit_or_rollback_requests_chain() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("idle_transaction_chaining").await;
        let (client, connection) = server.connect().await;

        // Act
        let commit = client
            .batch_execute("COMMIT AND CHAIN")
            .await
            .expect_err("chain requires an explicit transaction");
        let rollback = client
            .batch_execute("ROLLBACK AND CHAIN")
            .await
            .expect_err("chain requires an explicit transaction");
        let recovered: i32 = client
            .query_one("SELECT 7", &[])
            .await
            .expect("idle errors do not poison later work")
            .get(0);

        // Assert
        assert_eq!(db_error(&commit).code().code(), "25P01");
        assert_eq!(db_error(&rollback).code().code(), "25P01");
        assert_eq!(recovered, 7);
        drop(client);
        server.shutdown(connection).await;
    });
}
