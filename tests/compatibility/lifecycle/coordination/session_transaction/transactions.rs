#[test]
fn should_keep_two_key_transaction_advisory_locks_until_commit() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("two_key_xact_advisory_lock").await;
        let (owner, owner_connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin lock-owner transaction");
        owner
            .batch_execute("SELECT pg_advisory_xact_lock(71, 29)")
            .await
            .expect("acquire a two-key transaction advisory lock");

        // Act
        let blocked: bool = contender
            .query_one("SELECT pg_try_advisory_xact_lock(71, 29)", &[])
            .await
            .expect("try the two-key lock while the owner holds it")
            .try_get(0)
            .expect("contended two-key lock result");
        owner
            .batch_execute("COMMIT")
            .await
            .expect("release transaction advisory lock at commit");
        let available: bool = contender
            .query_one("SELECT pg_try_advisory_xact_lock(71, 29)", &[])
            .await
            .expect("acquire the two-key lock after owner commit")
            .try_get(0)
            .expect("available two-key lock result");
        contender
            .batch_execute("COMMIT")
            .await
            .expect("release the contender's transaction advisory lock");

        // Assert
        assert!(!blocked);
        assert!(available);

        drop(owner);
        drop(contender);
        server.shutdown(owner_connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
    });
}
