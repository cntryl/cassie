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
