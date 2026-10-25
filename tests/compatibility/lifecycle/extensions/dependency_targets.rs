use super::*;

#[test]
fn should_reject_pgcrypto_installation_until_extension_compatibility_is_implemented() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_installation_boundary").await;
        let (client, connection) = server.connect().await;

        // Act
        let result = client.batch_execute("CREATE EXTENSION pgcrypto").await;
        let error = result.expect_err("pgcrypto compatibility remains pending under #843");

        // Assert
        assert_eq!(db_error(&error).code().code(), "0A000");

        drop(client);
        server.shutdown(connection).await;
    });
}
