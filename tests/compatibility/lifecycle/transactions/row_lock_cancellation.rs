use super::*;
use std::time::Duration;

async fn observe_owner_locked_row_wait<T>(
    cassie: &cassie::app::Cassie,
    query: &mut tokio::task::JoinHandle<T>,
    baseline_running_queries: u64,
) -> bool {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if query.is_finished() {
                return false;
            }
            let running_queries = cassie.metrics()["runtime"]["running_queries"]
                .as_u64()
                .unwrap_or_default();
            if running_queries > baseline_running_queries {
                tokio::task::yield_now().await;
                return !query.is_finished()
                    && cassie.metrics()["runtime"]["running_queries"]
                        .as_u64()
                        .is_some_and(|running| running > baseline_running_queries);
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or(false)
}

async fn create_locked_row(client: &tokio_postgres::Client) {
    client
        .batch_execute("CREATE TABLE compat_cancel_lock_rows (id INT PRIMARY KEY, value TEXT)")
        .await
        .expect("create a row-lock cancellation fixture");
    client
        .batch_execute("INSERT INTO compat_cancel_lock_rows VALUES (1, 'before')")
        .await
        .expect("seed the row that the owner will lock");
}

async fn lock_fixture_row(client: &tokio_postgres::Client) {
    client
        .batch_execute("BEGIN")
        .await
        .expect("begin the row-lock owner transaction");
    client
        .query_one(
            "SELECT id FROM compat_cancel_lock_rows WHERE id = 1 FOR UPDATE",
            &[],
        )
        .await
        .expect("hold a row lock while the contender waits");
}

#[test]
fn should_return_57014_given_a_blocked_row_lock_waiter_when_cancel_request_arrives() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        support::use_local_storage();
        let path = support::data_dir("row_lock_cancel_request");
        let _directory = DataDirectory(path.clone());
        let crate::support_unique_conflict::Fixture {
            cassie,
            owner,
            contender: waiter,
            owner_connection,
            contender_connection: waiter_connection,
            server,
        } = crate::support_unique_conflict::start(&path).await;
        let waiter = std::sync::Arc::new(waiter);
        create_locked_row(&owner).await;
        lock_fixture_row(&owner).await;
        let baseline_running_queries = cassie.metrics()["runtime"]["running_queries"]
            .as_u64()
            .unwrap_or_default();
        let cancel = waiter.cancel_token();
        let query_client = std::sync::Arc::clone(&waiter);
        let mut blocked_update = tokio::spawn(async move {
            query_client
                .execute(
                    "UPDATE compat_cancel_lock_rows SET value = 'cancelled' WHERE id = 1",
                    &[],
                )
                .await
        });
        // The only new request is an UPDATE of the row still locked by owner.
        // Confirm it entered the runtime and remained pending under that lock.
        let waiter_reached_owner_lock =
            observe_owner_locked_row_wait(&cassie, &mut blocked_update, baseline_running_queries)
                .await;

        // Act
        cancel
            .cancel_query(NoTls)
            .await
            .expect("send a PostgreSQL CancelRequest to the lock waiter");
        let result = tokio::time::timeout(Duration::from_secs(5), &mut blocked_update)
            .await
            .expect("the cancelled row-lock wait should complete promptly")
            .expect("the row-lock waiter task should finish");
        let error = result.expect_err("the cancelled update must return an error");
        owner
            .batch_execute("COMMIT")
            .await
            .expect("release the row lock after cancellation");
        let error_code = db_error(&error).code().code().to_owned();
        drop((owner, waiter));
        waiter_connection.abort();
        let _ = waiter_connection.await;
        owner_connection.abort();
        let _ = owner_connection.await;
        server.abort();
        let _ = server.await;

        // Assert
        assert!(
            waiter_reached_owner_lock,
            "the UPDATE must remain active after entering the runtime while the owner holds its row lock"
        );
        assert_eq!(error_code, "57014");
    });
}

#[test]
fn should_release_waiter_state_given_a_cancelled_row_lock_request_when_the_session_is_reused() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        support::use_local_storage();
        let path = support::data_dir("row_lock_cancel_reuse");
        let _directory = DataDirectory(path.clone());
        let crate::support_unique_conflict::Fixture {
            cassie,
            owner,
            contender: waiter,
            owner_connection,
            contender_connection: waiter_connection,
            server,
        } = crate::support_unique_conflict::start(&path).await;
        let waiter = std::sync::Arc::new(waiter);
        create_locked_row(&owner).await;
        lock_fixture_row(&owner).await;
        let baseline_running_queries = cassie.metrics()["runtime"]["running_queries"]
            .as_u64()
            .unwrap_or_default();
        let cancel = waiter.cancel_token();
        let query_client = std::sync::Arc::clone(&waiter);
        let mut blocked_update = tokio::spawn(async move {
            query_client
                .execute(
                    "UPDATE compat_cancel_lock_rows SET value = 'cancelled' WHERE id = 1",
                    &[],
                )
                .await
        });
        // The only new request is an UPDATE of the row still locked by owner.
        // Confirm it entered the runtime and remained pending under that lock.
        let waiter_reached_owner_lock =
            observe_owner_locked_row_wait(&cassie, &mut blocked_update, baseline_running_queries)
                .await;

        // Act
        cancel
            .cancel_query(NoTls)
            .await
            .expect("send a PostgreSQL CancelRequest to the lock waiter");
        let result = tokio::time::timeout(Duration::from_secs(5), &mut blocked_update)
            .await
            .expect("the cancelled row-lock wait should complete promptly")
            .expect("the row-lock waiter task should finish");
        let error = result.expect_err("the cancelled update must return an error");
        owner
            .batch_execute("COMMIT")
            .await
            .expect("release the row lock after cancellation");
        let updated = waiter
            .execute(
                "UPDATE compat_cancel_lock_rows SET value = 'reused' WHERE id = 1",
                &[],
            )
            .await
            .expect("reuse the same session for a fresh update after cancellation");
        let value: String = waiter
            .query_one(
                "SELECT value FROM compat_cancel_lock_rows WHERE id = 1",
                &[],
            )
            .await
            .expect("read the row after the reused session update")
            .get(0);
        let error_code = db_error(&error).code().code().to_owned();
        drop((owner, waiter));
        waiter_connection.abort();
        let _ = waiter_connection.await;
        owner_connection.abort();
        let _ = owner_connection.await;
        server.abort();
        let _ = server.await;

        // Assert
        assert!(
            waiter_reached_owner_lock,
            "the UPDATE must remain active after entering the runtime while the owner holds its row lock"
        );
        assert_eq!(error_code, "57014");
        assert_eq!(updated, 1);
        assert_eq!(value, "reused");
    });
}
