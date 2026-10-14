use super::*;

#[test]
fn should_enable_pgcrypto_digest_after_create_extension() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pgcrypto_extension_identity").await;
        let (client, connection) = server.connect().await;

        // Act
        execute_statements(
            &client,
            &[
                "CREATE EXTENSION pgcrypto",
                "CREATE EXTENSION IF NOT EXISTS pgcrypto",
            ],
        )
        .await
        .expect("install the selected built-in crypto extension idempotently");
        let digest: String = client
            .query_one("SELECT encode(digest('wave4', 'sha256'), 'hex')", &[])
            .await
            .expect("call the extension function after installation")
            .try_get(0)
            .expect("SHA-256 digest");

        // Assert
        assert_eq!(
            digest,
            "4559fbac12073e340b8be2bf5bdb10bbdc4bf627fdf4976901b4a3637146c634"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
