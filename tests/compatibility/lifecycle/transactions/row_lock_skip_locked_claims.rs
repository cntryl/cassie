use super::*;

async fn hold_first_ordered_claim(owner: &tokio_postgres::Client) -> (i32, i32) {
    execute_statements(
        owner,
        &[
            "CREATE TABLE compat_skip_locked_claims (id INT PRIMARY KEY, ready BOOLEAN NOT NULL)",
            "INSERT INTO compat_skip_locked_claims VALUES (1, TRUE), (2, TRUE), (3, TRUE), (4, TRUE)",
            "BEGIN",
        ],
    )
    .await
    .expect("create ordered claim rows and begin the first owner transaction");
    let owner_lock = owner
        .query_one(
            "SELECT id FROM compat_skip_locked_claims WHERE ready \
             ORDER BY id LIMIT 1 FOR UPDATE",
            &[],
        )
        .await
        .expect("lock the first ready row")
        .get(0);
    let owner_claim = owner
        .query_one(
            "UPDATE compat_skip_locked_claims SET ready = FALSE \
             WHERE id = 1 AND ready RETURNING id",
            &[],
        )
        .await
        .expect("stage the first row claim")
        .get(0);
    (owner_lock, owner_claim)
}

async fn claim_next_ready_row(worker: &tokio_postgres::Client) -> (i32, i32) {
    let claimed_id = worker
        .query_one(
            "SELECT id FROM compat_skip_locked_claims WHERE ready \
             ORDER BY id LIMIT 1 FOR UPDATE SKIP LOCKED",
            &[],
        )
        .await
        .expect("worker should skip rows locked by other sessions")
        .get(0);
    let updated_id = worker
        .query_one(
            "UPDATE compat_skip_locked_claims SET ready = FALSE \
             WHERE id = $1 AND ready RETURNING id",
            &[&claimed_id],
        )
        .await
        .expect("stage the worker's ordered claim")
        .get(0);
    (claimed_id, updated_id)
}

#[test]
fn should_assign_ordered_skip_locked_claims_to_competing_workers() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("ordered_skip_locked_claims").await;
        let (owner, owner_connection) = server.connect().await;
        let (first_worker, first_connection) = server.connect().await;
        let (second_worker, second_connection) = server.connect().await;
        let (owner_lock, owner_claim) = hold_first_ordered_claim(&owner).await;
        first_worker
            .batch_execute("BEGIN")
            .await
            .expect("begin the first competing worker transaction");
        second_worker
            .batch_execute("BEGIN")
            .await
            .expect("begin the second competing worker transaction");

        // Act
        let (first_claim, first_update) = claim_next_ready_row(&first_worker).await;
        let (second_claim, second_update) = claim_next_ready_row(&second_worker).await;

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

#[test]
fn should_skip_conflicting_rows_for_each_row_lock_strength() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("skip_locked_row_lock_strengths").await;
        let (owner, owner_connection) = server.connect().await;
        let (worker, worker_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TABLE compat_skip_locked_strengths (id INT PRIMARY KEY)",
                "INSERT INTO compat_skip_locked_strengths VALUES (1),(2)",
                "BEGIN",
            ],
        )
        .await
        .expect("create two rows and begin the locking owner");
        let locked_id: i32 = owner
            .query_one(
                "SELECT id FROM compat_skip_locked_strengths WHERE id = 1 FOR UPDATE",
                &[],
            )
            .await
            .expect("hold a conflicting row lock on the first row")
            .get(0);
        worker
            .batch_execute("SET lock_timeout = '1s'")
            .await
            .expect("bound any lock wait if SKIP LOCKED is ignored");
        let modes = ["UPDATE", "NO KEY UPDATE", "SHARE", "KEY SHARE"];

        // Act
        let mut observations = Vec::with_capacity(modes.len());
        for mode in modes {
            let begin = worker.batch_execute("BEGIN").await;
            let query = format!(
                "SELECT id FROM compat_skip_locked_strengths ORDER BY id \
                 FOR {mode} SKIP LOCKED"
            );
            let ids = if begin.is_ok() {
                worker.query(&query, &[]).await.map(|rows| {
                    rows.iter()
                        .map(|row| row.get::<_, i32>(0))
                        .collect::<Vec<_>>()
                })
            } else {
                begin.map(|()| Vec::new())
            };
            let rollback = worker.batch_execute("ROLLBACK").await;
            observations.push((mode, ids, rollback));
        }
        owner
            .batch_execute("COMMIT")
            .await
            .expect("release the owner row lock");
        drop((owner, worker));
        server.shutdown(owner_connection).await;
        worker_connection.abort();
        let _ = worker_connection.await;

        // Assert
        assert_eq!(locked_id, 1);
        for (mode, ids, rollback) in observations {
            assert_eq!(
                ids.expect("SKIP LOCKED works for each row-lock strength"),
                vec![2],
                "FOR {mode} SKIP LOCKED must skip row 1 and return row 2"
            );
            rollback.expect("roll back each worker transaction and its row lock");
        }
    });
}

#[test]
fn should_not_skip_a_conflicting_table_lock_with_skip_locked() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("skip_locked_table_lock_boundary").await;
        let (owner, owner_connection) = server.connect().await;
        let (worker, worker_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TABLE compat_skip_locked_table_boundary (id INT PRIMARY KEY)",
                "INSERT INTO compat_skip_locked_table_boundary VALUES (1),(2)",
                "BEGIN",
                "LOCK TABLE compat_skip_locked_table_boundary IN EXCLUSIVE MODE",
            ],
        )
        .await
        .expect("hold an EXCLUSIVE table lock against the SELECT row lock");
        worker
            .batch_execute("SET lock_timeout = '100ms'; BEGIN")
            .await
            .expect("bound the relation-lock wait and begin the worker");

        // Act
        let blocked_query = worker
            .query(
                "SELECT id FROM compat_skip_locked_table_boundary \
                 ORDER BY id FOR UPDATE SKIP LOCKED",
                &[],
            )
            .await;
        let failed_followup = worker.query_one("SELECT 1", &[]).await;
        worker
            .batch_execute("ROLLBACK")
            .await
            .expect("recover the worker after its table-lock timeout");
        owner
            .batch_execute("COMMIT")
            .await
            .expect("release the table lock");
        let retry = worker
            .query(
                "SELECT id FROM compat_skip_locked_table_boundary \
                 ORDER BY id FOR UPDATE SKIP LOCKED",
                &[],
            )
            .await;
        let retry_ids = retry.map(|rows| {
            rows.iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>()
        });
        drop((owner, worker));
        server.shutdown(owner_connection).await;
        worker_connection.abort();
        let _ = worker_connection.await;

        // Assert
        let blocked_query = blocked_query
            .expect_err("SKIP LOCKED skips row locks, but relation-lock acquisition still waits");
        assert_eq!(db_error(&blocked_query).code().code(), "55P03");
        assert_eq!(
            db_error(&failed_followup.expect_err("a lock timeout aborts the transaction"))
                .code()
                .code(),
            "25P02"
        );
        assert_eq!(
            retry_ids.expect("retry after releasing the table lock"),
            vec![1, 2]
        );
    });
}
