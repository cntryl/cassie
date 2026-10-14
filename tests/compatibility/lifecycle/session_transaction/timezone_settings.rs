use super::*;

#[test]
fn should_restore_startup_timezone_after_reset() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("session_timezone_reset").await;
        let (client, connection) = server.connect().await;
        let startup_zone: String = client
            .query_one("SHOW timezone", &[])
            .await
            .expect("read the session startup timezone")
            .try_get(0)
            .expect("startup timezone");
        client
            .batch_execute("SET timezone = 'America/Los_Angeles'")
            .await
            .expect("set a session timezone");

        // Act
        let configured_zone: String = client
            .query_one("SHOW timezone", &[])
            .await
            .expect("read the configured timezone")
            .try_get(0)
            .expect("configured timezone");
        client
            .batch_execute("RESET timezone")
            .await
            .expect("restore the startup timezone");
        let reset_zone: String = client
            .query_one("SHOW timezone", &[])
            .await
            .expect("read the reset timezone")
            .try_get(0)
            .expect("reset timezone");

        // Assert
        assert_eq!(configured_zone, "America/Los_Angeles");
        assert_eq!(reset_zone, startup_zone);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_restore_set_local_timezone_when_rolling_back_to_savepoint() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("timezone_savepoint_restore").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("SET timezone = 'America/Los_Angeles'")
            .await
            .expect("set the session timezone baseline");
        let session_zone: String = client
            .query_one("SHOW timezone", &[])
            .await
            .expect("read the session timezone baseline")
            .get(0);

        // Act
        client
            .batch_execute(
                "BEGIN; SET LOCAL timezone = 'America/New_York'; \
                 SAVEPOINT compat_timezone_checkpoint; SET LOCAL timezone = 'UTC'",
            )
            .await
            .expect("change a local timezone before and after a savepoint");
        let nested_zone: String = client
            .query_one("SHOW timezone", &[])
            .await
            .expect("read the nested local timezone")
            .get(0);
        client
            .batch_execute("ROLLBACK TO SAVEPOINT compat_timezone_checkpoint")
            .await
            .expect("roll back the nested SET LOCAL");
        let savepoint_zone: String = client
            .query_one("SHOW timezone", &[])
            .await
            .expect("read the local timezone restored by savepoint rollback")
            .get(0);
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the transaction-local timezone");
        let restored_zone: String = client
            .query_one("SHOW timezone", &[])
            .await
            .expect("read the session timezone after transaction rollback")
            .get(0);

        // Assert
        assert_eq!(session_zone, "America/Los_Angeles");
        assert_eq!(nested_zone, "UTC");
        assert_eq!(savepoint_zone, "America/New_York");
        assert_eq!(restored_zone, session_zone);

        drop(client);
        server.shutdown(connection).await;
    });
}
