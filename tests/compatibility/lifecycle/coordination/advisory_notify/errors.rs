#[test]
fn should_reject_oversized_notification_payload_with_sqlstate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("notify_payload_limit").await;
        let (client, connection) = server.connect().await;

        // Act
        let boundary = client
            .query_one(
                "SELECT pg_notify('compat_payload_limit', repeat('x', 7999))",
                &[],
            )
            .await
            .expect("accept a notification payload of 7999 bytes");
        let too_long = client
            .query_one(
                "SELECT pg_notify('compat_payload_limit', repeat('x', 8000))",
                &[],
            )
            .await
            .expect_err("reject a notification payload of 8000 bytes");
        let recovered: i32 = client
            .query_one("SELECT 1", &[])
            .await
            .expect("reuse the session after payload rejection")
            .get(0);

        // Assert
        assert_eq!(boundary.columns()[0].type_().oid(), 2278);
        assert_eq!(db_error(&too_long).code().code(), "22023");
        assert_eq!(recovered, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
