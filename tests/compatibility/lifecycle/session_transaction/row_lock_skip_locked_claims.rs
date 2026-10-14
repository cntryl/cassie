use super::*;

#[test]
fn should_assign_ordered_skip_locked_claims_to_competing_workers() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("ordered_skip_locked_claims").await;
        let (owner, owner_connection) = server.connect().await;
        let (first_worker, first_connection) = server.connect().await;
        let (second_worker, second_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TABLE compat_skip_locked_claims (id INT PRIMARY KEY, ready BOOLEAN NOT NULL)",
                "INSERT INTO compat_skip_locked_claims VALUES (1, TRUE), (2, TRUE), (3, TRUE), (4, TRUE)",
                "BEGIN",
            ],
        )
        .await
        .expect("create ordered claim rows and begin the first owner transaction");
        let owner_lock: i32 = owner
            .query_one(
                "SELECT id FROM compat_skip_locked_claims WHERE ready \
                 ORDER BY id LIMIT 1 FOR UPDATE",
                &[],
            )
            .await
            .expect("lock the first ready row")
            .try_get(0)
            .expect("owner row id");
        let owner_claim: i32 = owner
            .query_one(
                "UPDATE compat_skip_locked_claims SET ready = FALSE \
                 WHERE id = 1 AND ready RETURNING id",
                &[],
            )
            .await
            .expect("stage the first row claim")
            .try_get(0)
            .expect("owner claim id");
        first_worker
            .batch_execute("BEGIN")
            .await
            .expect("begin the first competing worker transaction");
        second_worker
            .batch_execute("BEGIN")
            .await
            .expect("begin the second competing worker transaction");

        // Act
        let first_claim: i32 = first_worker
            .query_one(
                "SELECT id FROM compat_skip_locked_claims WHERE ready \
                 ORDER BY id LIMIT 1 FOR UPDATE SKIP LOCKED",
                &[],
            )
            .await
            .expect("first worker should skip the owner's locked row")
            .try_get(0)
            .expect("first worker claim id");
        let first_update: i32 = first_worker
            .query_one(
                "UPDATE compat_skip_locked_claims SET ready = FALSE \
                 WHERE id = 2 AND ready RETURNING id",
                &[],
            )
            .await
            .expect("stage the first worker's claim")
            .try_get(0)
            .expect("first worker updated id");
        let second_claim: i32 = second_worker
            .query_one(
                "SELECT id FROM compat_skip_locked_claims WHERE ready \
                 ORDER BY id LIMIT 1 FOR UPDATE SKIP LOCKED",
                &[],
            )
            .await
            .expect("second worker should skip both rows locked by other sessions")
            .try_get(0)
            .expect("second worker claim id");
        let second_update: i32 = second_worker
            .query_one(
                "UPDATE compat_skip_locked_claims SET ready = FALSE \
                 WHERE id = 3 AND ready RETURNING id",
                &[],
            )
            .await
            .expect("stage the second worker's claim")
            .try_get(0)
            .expect("second worker updated id");

        owner
            .batch_execute("COMMIT")
            .await
            .expect("commit the first claim owner");
        first_worker
            .batch_execute("COMMIT")
            .await
            .expect("commit the first worker claim");
        second_worker
            .batch_execute("COMMIT")
            .await
            .expect("commit the second worker claim");
        let remaining_ready = owner
            .query(
                "SELECT id FROM compat_skip_locked_claims WHERE ready ORDER BY id",
                &[],
            )
            .await
            .expect("read rows not claimed by any worker")
            .iter()
            .map(|row| row.get::<_, i32>(0))
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(owner_lock, 1);
        assert_eq!(owner_claim, 1);
        assert_eq!(first_claim, 2);
        assert_eq!(first_update, 2);
        assert_eq!(second_claim, 3);
        assert_eq!(second_update, 3);
        assert_eq!(remaining_ready, vec![4]);

        drop((owner, first_worker, second_worker));
        server.shutdown(owner_connection).await;
        first_connection.abort();
        let _ = first_connection.await;
        second_connection.abort();
        let _ = second_connection.await;
    });
}
