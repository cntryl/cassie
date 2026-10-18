#[test]
fn should_return_void_from_advisory_unlock_all() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("advisory_unlock_all").await;
        let (client, connection) = server.connect().await;

        // Act
        let statement = client
            .prepare("SELECT pg_advisory_unlock_all()")
            .await
            .expect("prepare the advisory unlock-all function");
        let row = client
            .query_one("SELECT pg_advisory_unlock_all()::TEXT", &[])
            .await
            .expect("pg_advisory_unlock_all returns PostgreSQL VOID");

        // Assert
        assert_eq!(statement.columns()[0].type_().oid(), 2278);
        assert_eq!(row.get::<_, String>(0), "");

        drop(client);
        server.shutdown(connection).await;
    });
}
