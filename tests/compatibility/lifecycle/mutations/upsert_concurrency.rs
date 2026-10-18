use super::*;
use std::time::Duration;

async fn create_upsert_fixture(client: &tokio_postgres::Client) {
    client
        .batch_execute(
            "CREATE TABLE compat_upsert_wait_rows ( \
               id INT PRIMARY KEY, inserted_by TEXT NOT NULL, update_count INT NOT NULL)",
        )
        .await
        .expect("create a row that distinguishes the conflict and insert paths");
    client
        .batch_execute("BEGIN")
        .await
        .expect("begin the owner transaction");
    client
        .batch_execute("INSERT INTO compat_upsert_wait_rows VALUES (1, 'owner', 0)")
        .await
        .expect("hold the unique key in an uncommitted transaction");
}

fn start_waiting_upsert(
    waiter: tokio_postgres::Client,
) -> tokio::task::JoinHandle<Result<tokio_postgres::Row, tokio_postgres::Error>> {
    tokio::spawn(async move {
        waiter
            .query_one(
                "INSERT INTO compat_upsert_wait_rows VALUES (1, 'waiter', 0) \
                 ON CONFLICT (id) DO UPDATE \
                   SET update_count = compat_upsert_wait_rows.update_count + 1 \
                 RETURNING inserted_by, update_count",
                &[],
            )
            .await
    })
}

async fn assert_upsert_waited_for_owner(
    owner: &tokio_postgres::Client,
    waiter: tokio_postgres::Client,
    commit_owner: bool,
) -> (bool, Result<tokio_postgres::Row, tokio_postgres::Error>) {
    let mut pending = start_waiting_upsert(waiter);
    let early_result = tokio::time::timeout(Duration::from_millis(250), &mut pending)
        .await
        .ok();
    let completed_before_owner = early_result.is_some();
    owner
        .batch_execute(if commit_owner { "COMMIT" } else { "ROLLBACK" })
        .await
        .expect("finish the unique-key owner transaction");
    let result = match early_result {
        Some(result) => result.expect("waiting upsert task should finish"),
        None => tokio::time::timeout(Duration::from_secs(5), &mut pending)
            .await
            .expect("waiting upsert should finish after the owner transaction")
            .expect("waiting upsert task should finish"),
    };
    (completed_before_owner, result)
}

#[test]
fn should_recheck_the_conflicting_row_given_an_upsert_waiter_when_the_owner_commits() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("upsert_wait_owner_commit").await;
        let (owner, owner_connection) = server.connect().await;
        let (waiter, waiter_connection) = server.connect().await;
        create_upsert_fixture(&owner).await;

        // Act
        let (completed_before_owner_commit, returned) =
            assert_upsert_waited_for_owner(&owner, waiter, true).await;
        let rows = owner
            .query(
                "SELECT inserted_by, update_count FROM compat_upsert_wait_rows WHERE id = 1",
                &[],
            )
            .await
            .expect("read the committed conflict row");

        // Assert
        let returned = returned.expect("upsert should update the committed conflicting row");
        assert!(!completed_before_owner_commit);
        assert_eq!(returned.get::<_, String>(0), "owner");
        assert_eq!(returned.get::<_, i32>(1), 1);
        assert_eq!(rows[0].get::<_, String>(0), "owner");
        assert_eq!(rows[0].get::<_, i32>(1), 1);
        drop(owner);
        server.shutdown(owner_connection).await;
        waiter_connection.abort();
        let _ = waiter_connection.await;
    });
}

#[test]
fn should_insert_the_waiting_row_given_an_upsert_waiter_when_the_owner_rolls_back() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("upsert_wait_owner_rollback").await;
        let (owner, owner_connection) = server.connect().await;
        let (waiter, waiter_connection) = server.connect().await;
        create_upsert_fixture(&owner).await;

        // Act
        let (completed_before_owner_rollback, returned) =
            assert_upsert_waited_for_owner(&owner, waiter, false).await;
        let rows = owner
            .query(
                "SELECT inserted_by, update_count FROM compat_upsert_wait_rows WHERE id = 1",
                &[],
            )
            .await
            .expect("read the row inserted after the owner rolled back");

        // Assert
        let returned = returned.expect("upsert should insert after the owner rolls back");
        assert!(!completed_before_owner_rollback);
        assert_eq!(returned.get::<_, String>(0), "waiter");
        assert_eq!(returned.get::<_, i32>(1), 0);
        assert_eq!(rows[0].get::<_, String>(0), "waiter");
        assert_eq!(rows[0].get::<_, i32>(1), 0);
        drop(owner);
        server.shutdown(owner_connection).await;
        waiter_connection.abort();
        let _ = waiter_connection.await;
    });
}
