use super::*;

#[test]
fn should_unlisten_all_active_channels_for_only_the_current_session() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("unlisten_all_active_channels").await;
        let (listener, listener_connection) = server.connect().await;
        let (other_listener, other_listener_connection) = server.connect().await;
        listener
            .batch_execute("LISTEN compat_unlisten_alpha; LISTEN compat_unlisten_beta")
            .await
            .expect("subscribe the current session to two channels");
        other_listener
            .batch_execute("LISTEN compat_unlisten_alpha")
            .await
            .expect("subscribe the second session to one shared channel");

        // Act
        listener
            .batch_execute("UNLISTEN *")
            .await
            .expect("remove all active subscriptions for the current session");
        let listener_channels = listener
            .query("SELECT pg_listening_channels() ORDER BY 1", &[])
            .await
            .expect("inspect channels after UNLISTEN *")
            .iter()
            .map(|row| row.get::<_, String>(0))
            .collect::<Vec<_>>();
        let other_listener_channels = other_listener
            .query("SELECT pg_listening_channels() ORDER BY 1", &[])
            .await
            .expect("inspect the independent session's channels")
            .iter()
            .map(|row| row.get::<_, String>(0))
            .collect::<Vec<_>>();
        // Assert
        assert_eq!(listener_channels, Vec::<String>::new());
        assert_eq!(other_listener_channels, vec!["compat_unlisten_alpha"]);

        drop((listener, other_listener));
        server.shutdown(listener_connection).await;
        other_listener_connection.abort();
        let _ = other_listener_connection.await;
    });
}
