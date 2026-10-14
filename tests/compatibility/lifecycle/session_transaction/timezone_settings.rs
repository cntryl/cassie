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
