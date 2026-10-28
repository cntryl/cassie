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
        let (reset, reset_value) = if configured_value.as_ref().is_some_and(Result::is_ok) {
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
