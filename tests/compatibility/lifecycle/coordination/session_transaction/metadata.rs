#[test]
fn should_return_void_from_pg_notify_without_a_listener() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pg_notify_function").await;
        let (client, connection) = server.connect().await;

        // Act
        let notification = client
            .query_one(
                "SELECT pg_notify('compat_pg_notify_function', 'payload-18')::TEXT",
                &[],
            )
            .await;

        // Assert
        let notification = notification.expect("pg_notify returns PostgreSQL VOID");
        assert_eq!(notification.get::<_, String>(0), "");

        drop(client);
        server.shutdown(connection).await;
    });
}
