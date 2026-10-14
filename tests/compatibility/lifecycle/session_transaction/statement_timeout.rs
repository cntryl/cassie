use super::*;

#[test]
fn should_accept_statement_timeout_setting() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("statement_timeout_setting").await;
        let (client, connection) = server.connect().await;
        let configured = client.batch_execute("SET statement_timeout = '50ms'").await;

        // Act
        let configured_value = if configured.is_ok() {
            Some(client.query_one("SHOW statement_timeout", &[]).await)
        } else {
            None
        };
        let (reset, reset_value) = if configured_value.as_ref().is_some_and(|value| value.is_ok()) {
            let reset = client.batch_execute("RESET statement_timeout").await;
            let reset_value = if reset.is_ok() {
                Some(client.query_one("SHOW statement_timeout", &[]).await)
            } else {
                None
            };
            (Some(reset), reset_value)
        } else {
            (None, None)
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;

        assert!(
            configured.is_ok(),
            "statement_timeout should be accepted: {configured:?}"
        );
        let configured_value = configured_value
            .expect("read statement_timeout after setting it")
            .expect("SHOW statement_timeout after SET");
        assert_eq!(configured_value.columns()[0].type_().oid(), 25);
        assert_eq!(
            configured_value
                .try_get::<_, String>(0)
                .expect("statement timeout text"),
            "50ms"
        );
        reset
            .expect("RESET statement_timeout should be attempted")
            .expect("RESET statement_timeout");
        assert_eq!(
            reset_value
                .expect("read statement_timeout after reset")
                .expect("SHOW statement_timeout after RESET")
                .try_get::<_, String>(0)
                .expect("reset statement timeout text"),
            "0"
        );
    });
}

#[test]
fn should_restore_session_lock_timeout_after_local_setting_rollback() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("local_lock_timeout_rollback").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("SET lock_timeout = '17s'")
            .await
            .expect("set the session timeout before the transaction");
        client
            .batch_execute("BEGIN READ ONLY")
            .await
            .expect("begin a read-only transaction");
        client
            .batch_execute("SET LOCAL lock_timeout = '5s'")
            .await
            .expect("override the session timeout for this transaction");
        let transaction_timeout: String = client
            .query_one("SHOW lock_timeout", &[])
            .await
            .expect("read the transaction-local timeout")
            .try_get(0)
            .expect("transaction-local timeout text");

        // Act
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the read-only transaction");
        let session_timeout: String = client
            .query_one("SHOW lock_timeout", &[])
            .await
            .expect("read the session timeout restored by rollback")
            .try_get(0)
            .expect("restored session timeout text");

        // Assert
        assert_eq!(transaction_timeout, "5s");
        assert_eq!(session_timeout, "17s");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_cancel_overdue_statement_with_sqlstate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("statement_timeout_cancellation").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("SET statement_timeout = '25ms'")
            .await
            .expect("set a short statement timeout");

        // Act
        let cancelled = client
            .query_one("SELECT pg_sleep(0.2)", &[])
            .await
            .expect_err("pg_sleep exceeding statement_timeout must be cancelled");
        client
            .batch_execute("RESET statement_timeout")
            .await
            .expect("reset the statement timeout after cancellation");
        let recovered: i32 = client
            .query_one("SELECT 1", &[])
            .await
            .expect("reuse the session after timeout cancellation")
            .get(0);

        // Assert
        assert_eq!(db_error(&cancelled).code().code(), "57014");
        assert_eq!(recovered, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_abort_lock_timed_out_transaction_then_recover_after_rollback() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("lock_timeout_recovery").await;
        let (owner, owner_connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        owner
            .batch_execute("CREATE TABLE compat_lock_timeout_rows (id INT PRIMARY KEY, value INT)")
            .await
            .expect("create the lock timeout fixture");
        owner
            .batch_execute("INSERT INTO compat_lock_timeout_rows VALUES (1, 1)")
            .await
            .expect("seed the lock timeout fixture");
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin the owner transaction");
        owner
            .batch_execute("UPDATE compat_lock_timeout_rows SET value = 2 WHERE id = 1")
            .await
            .expect("hold a row update in the owner transaction");
        contender
            .batch_execute("SET lock_timeout = '50ms'; BEGIN")
            .await
            .expect("set a bounded lock timeout and start a contender transaction");

        // Act
        let mut blocked_update = tokio::spawn(async move {
            let result = contender
                .execute(
                    "UPDATE compat_lock_timeout_rows SET value = 3 WHERE id = 1",
                    &[],
                )
                .await;
            (contender, result)
        });
        let bounded_wait = tokio::time::timeout(Duration::from_secs(2), &mut blocked_update).await;
        let owner_committed_for_cleanup = bounded_wait.is_err();
        if owner_committed_for_cleanup {
            owner
                .batch_execute("COMMIT")
                .await
                .expect("release the owner lock if the configured timeout did not fire");
        }
        let (contender, update_result) = match bounded_wait {
            Ok(joined) => joined.expect("contending UPDATE task should complete"),
            Err(_) => blocked_update
                .await
                .expect("contending UPDATE should complete after owner releases its lock"),
        };
        let lock_sqlstate = update_result
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_string());
        let followup_sqlstate = contender
            .query_one("SELECT 1", &[])
            .await
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_string());
        contender
            .batch_execute("ROLLBACK")
            .await
            .expect("recover the contender session with ROLLBACK");
        if !owner_committed_for_cleanup {
            owner
                .batch_execute("COMMIT")
                .await
                .expect("release the owner row lock");
        }
        let retry_rows = contender
            .execute(
                "UPDATE compat_lock_timeout_rows SET value = 4 WHERE id = 1",
                &[],
            )
            .await
            .expect("retry the update after the owner and failed transaction are finished");
        contender
            .batch_execute("RESET lock_timeout")
            .await
            .expect("restore the session lock timeout");

        // Assert
        assert_eq!(lock_sqlstate.as_deref(), Some("55P03"));
        assert_eq!(followup_sqlstate.as_deref(), Some("25P02"));
        assert_eq!(retry_rows, 1);

        drop((owner, contender));
        server.shutdown(owner_connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
    });
}
