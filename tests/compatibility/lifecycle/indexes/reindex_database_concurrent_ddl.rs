use super::*;

async fn quoted_current_database(client: &tokio_postgres::Client) -> String {
    let database_name: String = client
        .query_one("SELECT current_database()", &[])
        .await
        .expect("read the current database name")
        .try_get(0)
        .expect("database name");
    format!("\"{}\"", database_name.replace('"', "\"\""))
}

#[test]
fn should_reject_concurrent_system_reindex_with_feature_not_supported() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reindex_system_concurrently_rejected").await;
        let (client, connection) = server.connect().await;
        let database_identifier = quoted_current_database(&client).await;

        // Act
        let rejected = client
            .batch_execute(&format!(
                "REINDEX (VERBOSE FALSE, CONCURRENTLY TRUE) \
                 SYSTEM {database_identifier}"
            ))
            .await
            .expect_err("concurrent REINDEX is unsupported");

        // Assert
        assert_eq!(db_error(&rejected).code().code(), "0A000");

        drop(client);
        server.shutdown(connection).await;
    });
}
