#[test]
fn should_publish_sequence_restart_options_through_sql() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sequence_options").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE SEQUENCE compat_order_sequence START WITH 7 INCREMENT BY 3 \
                   MINVALUE 1 MAXVALUE 10 CYCLE",
            )
            .await
            .expect("create configured sequence");

        // Act
        let values = client
            .query(
                "SELECT nextval('compat_order_sequence'), currval('compat_order_sequence')",
                &[],
            )
            .await
            .expect("read next and current sequence values");
        client
            .batch_execute("ALTER SEQUENCE compat_order_sequence RESTART WITH 4")
            .await
            .expect("restart sequence at selected value");
        let restarted: i64 = client
            .query_one("SELECT nextval('compat_order_sequence')", &[])
            .await
            .expect("read restarted sequence value")
            .try_get(0)
            .expect("sequence value");

        // Assert
        assert_eq!(values[0].get::<_, i64>(0), 7);
        assert_eq!(values[0].get::<_, i64>(1), 7);
        assert_eq!(restarted, 4);

        drop(client);
        server.shutdown(connection).await;
    });
}
