//! PostgreSQL lock and safe-snapshot blocker functions (#817).

use std::time::{Duration, Instant};

use crate::support_catalog_sessions as fixture;

#[test]
fn should_report_lock_blockers_given_a_waiting_session_when_pg_blocking_pids_is_called() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let fixture = fixture::start("issue-817-blocking-pids").await;
        let owner = fixture::connect(&fixture).await;
        let waiter = fixture::connect(&fixture).await;
        let observer = fixture::connect(&fixture).await;
        owner
            .client
            .batch_execute(
                "CREATE TABLE issue_817_blocked_row (id INTEGER PRIMARY KEY, value INTEGER); \
                 INSERT INTO issue_817_blocked_row VALUES (1, 0)",
            )
            .await
            .expect("create a row to lock");
        let owner_pid: i32 = owner
            .client
            .query_one("SELECT pg_backend_pid()", &[])
            .await
            .expect("read lock owner pid")
            .get(0);
        let waiter_pid: i32 = waiter
            .client
            .query_one("SELECT pg_backend_pid()", &[])
            .await
            .expect("read blocked session pid")
            .get(0);
        owner
            .client
            .batch_execute("BEGIN")
            .await
            .expect("begin the lock owner transaction");
        let locked = owner
            .client
            .query_one(
                "SELECT id FROM issue_817_blocked_row WHERE id = 1 FOR UPDATE",
                &[],
            )
            .await
            .expect("hold a row lock in the owner session");
        assert_eq!(locked.get::<_, i32>(0), 1);
        let waiting_client = waiter.client.clone();
        waiter
            .client
            .batch_execute("BEGIN")
            .await
            .expect("begin the waiting transaction");
        let blocked_update = tokio::spawn(async move {
            waiting_client
                .execute(
                    "UPDATE issue_817_blocked_row SET value = value + 1 WHERE id = 1",
                    &[],
                )
                .await
        });

        // Act
        let sql = format!("SELECT {owner_pid} = ANY(pg_catalog.pg_blocking_pids({waiter_pid}))");
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let row = observer
                .client
                .query_one(&sql, &[])
                .await
                .expect("report the transaction blocking the waiting backend");
            if row.get::<_, bool>(0) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the waiter should report the owner after its row-lock request is queued"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        owner
            .client
            .batch_execute("COMMIT")
            .await
            .expect("release the row lock after observing the blocker");
        let update_rows = blocked_update
            .await
            .expect("blocked update task")
            .expect("complete the blocked update after lock release");

        // Assert
        assert_eq!(update_rows, 1);
        fixture::stop(fixture, vec![owner, waiter, observer]).await;
    });
}

#[test]
fn should_report_snapshot_blockers_given_a_deferrable_snapshot_waiter_when_safe_snapshot_blockers_are_queried(
) {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let fixture = fixture::start("issue-817-safe-snapshot-blockers").await;
        let writer = fixture::connect(&fixture).await;
        let waiter = fixture::connect(&fixture).await;
        let observer = fixture::connect(&fixture).await;
        writer
            .client
            .batch_execute(
                "CREATE TABLE issue_817_snapshot_rows (id INTEGER); \
                 INSERT INTO issue_817_snapshot_rows VALUES (1)",
            )
            .await
            .expect("create a row for the serializable writer");
        let serializable_writer_pid: i32 = writer
            .client
            .query_one("SELECT pg_backend_pid()", &[])
            .await
            .expect("read writer backend pid")
            .get(0);
        let snapshot_waiter_pid: i32 = waiter
            .client
            .query_one("SELECT pg_backend_pid()", &[])
            .await
            .expect("read safe-snapshot waiter pid")
            .get(0);
        writer
            .client
            .batch_execute(
                "BEGIN ISOLATION LEVEL SERIALIZABLE; \
                 INSERT INTO issue_817_snapshot_rows VALUES (2)",
            )
            .await
            .expect("keep a serializable writer active");
        waiter
            .client
            .batch_execute("BEGIN ISOLATION LEVEL SERIALIZABLE READ ONLY DEFERRABLE")
            .await
            .expect("begin a deferrable read-only transaction");
        let waiting_client = waiter.client.clone();
        let waiting_snapshot = tokio::spawn(async move {
            waiting_client
                .query_one("SELECT count(*) FROM issue_817_snapshot_rows", &[])
                .await
        });

        // Act
        let sql = format!(
            "SELECT {serializable_writer_pid} = ANY(\
                 pg_catalog.pg_safe_snapshot_blocking_pids({snapshot_waiter_pid}))"
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let row = observer
                .client
                .query_one(&sql, &[])
                .await
                .expect("report the transaction blocking a safe snapshot");
            if row.get::<_, bool>(0) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the read-only deferrable query should wait for the active writer"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        writer
            .client
            .batch_execute("COMMIT")
            .await
            .expect("finish the writer after observing the safe-snapshot blocker");
        let snapshot_rows = waiting_snapshot
            .await
            .expect("safe-snapshot query task")
            .expect("complete the waiting query after the writer commits");

        // Assert
        assert_eq!(snapshot_rows.get::<_, i64>(0), 2);
        fixture::stop(fixture, vec![writer, waiter, observer]).await;
    });
}
