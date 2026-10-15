use super::*;

#[test]
fn should_return_zero_trigger_depth_outside_a_trigger() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("trigger_depth_outside_trigger").await;
        let (client, connection) = server.connect().await;

        // Act
        let row = client
            .query_one("SELECT pg_trigger_depth()", &[])
            .await
            .expect("read the current trigger nesting depth");

        // Assert
        assert_eq!(row.columns()[0].type_().oid(), 23);
        assert_eq!(row.get::<_, i32>(0), 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
