use super::*;

#[test]
fn should_distinguish_advisory_lock_key_spaces() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("advisory_lock_key_spaces").await;
        let (single_key_owner, owner_connection) = server.connect().await;
        let (two_key_owner, two_key_connection) = server.connect().await;
        single_key_owner
            .query_one("SELECT pg_advisory_lock(4294967297::BIGINT)", &[])
            .await
            .expect("acquire the one-BIGINT-key lock");

        // Act
        let two_key_acquired: bool = two_key_owner
            .query_one("SELECT pg_try_advisory_lock(1, 1)", &[])
            .await
            .expect("try the two-INT-key lock with matching 64-bit bits")
            .get(0);
        let single_key_still_held: bool = two_key_owner
            .query_one("SELECT pg_try_advisory_lock(4294967297::BIGINT)", &[])
            .await
            .expect("probe the one-key lock from the second session")
            .get(0);
        let two_key_released: bool = two_key_owner
            .query_one("SELECT pg_advisory_unlock(1, 1)", &[])
            .await
            .expect("release the acquired two-key lock")
            .get(0);
        let single_key_released: bool = single_key_owner
            .query_one("SELECT pg_advisory_unlock(4294967297::BIGINT)", &[])
            .await
            .expect("release the one-key lock")
            .get(0);
        let single_key_acquired_after_release: bool = two_key_owner
            .query_one("SELECT pg_try_advisory_lock(4294967297::BIGINT)", &[])
            .await
            .expect("acquire the one-key lock after its owner releases it")
            .get(0);
        let single_key_cleanup: bool = two_key_owner
            .query_one("SELECT pg_advisory_unlock(4294967297::BIGINT)", &[])
            .await
            .expect("release the final one-key lock")
            .get(0);

        // Assert
        assert!(two_key_acquired);
        assert!(!single_key_still_held);
        assert!(two_key_released);
        assert!(single_key_released);
        assert!(single_key_acquired_after_release);
        assert!(single_key_cleanup);

        drop((single_key_owner, two_key_owner));
        server.shutdown(owner_connection).await;
        two_key_connection.abort();
        let _ = two_key_connection.await;
    });
}
