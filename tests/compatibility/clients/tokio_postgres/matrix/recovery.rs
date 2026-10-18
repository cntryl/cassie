#[test]
fn should_recover_after_a_syntax_error_with_tokio_postgres() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("error_recovery").await;
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("connect should complete within the timeout");

        // Act
        let first = client.query_one("SELECT * FROM", &[]).await;
        let second = client
            .query_one("SELECT version()", &[])
            .await
            .expect("connection should recover after a syntax error");

        // Assert
        let first = first.expect_err("syntax error should be rejected");
        let db_error = db_error(&first);
        assert_eq!(db_error.code().code(), "42601");
        let version: String = second.try_get(0).expect("version column");
        assert!(version.starts_with("PostgreSQL 16.0 compatible Cassie "));

        drop(client);
        server.shutdown(connection).await;
    });
}
