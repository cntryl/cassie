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

async fn wait_for_upsert_boundary<T>(
    server: &CompatibilityServer,
    pending: &tokio::task::JoinHandle<T>,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if server.cassie.metrics()["transactions"]["unique_waiters"]
                .as_u64()
                .is_some_and(|count| count > 0)
            {
                break;
            }
            assert!(
                !pending.is_finished(),
                "upsert completed before acquiring its unique-owner wait"
            );
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("waiter reaches the owned-key boundary");
}

async fn assert_upsert_waited_for_owner(
    server: &CompatibilityServer,
    owner: &tokio_postgres::Client,
    waiter: tokio_postgres::Client,
    commit_owner: bool,
) -> (bool, Result<tokio_postgres::Row, tokio_postgres::Error>) {
    let mut pending = start_waiting_upsert(waiter);
    wait_for_upsert_boundary(server, &pending).await;
    let completed_before_owner = pending.is_finished();
    owner
        .batch_execute(if commit_owner { "COMMIT" } else { "ROLLBACK" })
        .await
        .expect("finish the unique-key owner transaction");
    let result = tokio::time::timeout(Duration::from_secs(5), &mut pending)
        .await
        .expect("upsert finishes after owner resolution")
        .expect("upsert task finishes");
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
            assert_upsert_waited_for_owner(&server, &owner, waiter, true).await;
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
            assert_upsert_waited_for_owner(&server, &owner, waiter, false).await;
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

#[test]
fn should_evaluate_upsert_defaults_once_given_owner_resolution_when_returning_final_rows() {
    // Arrange
    current_thread_runtime().block_on(async {
        for (commit, explicit) in [(true, false), (false, false), (true, true), (false, true)] {
            let server = CompatibilityServer::start("upsert_default_once").await;
            let (owner, owner_connection) = server.connect().await;
            let (waiter, waiter_connection) = server.connect().await;
            for sql in ["CREATE TABLE upsert_default_rows (id SERIAL PRIMARY KEY, label TEXT UNIQUE, marker INT)", "BEGIN", "INSERT INTO upsert_default_rows(label,marker) VALUES ('same',0)"] {
                owner.batch_execute(sql).await.expect("owner staged serial row");
            }
            let sql = if explicit { "INSERT INTO upsert_default_rows(id,label,marker) VALUES (DEFAULT,'same',0) ON CONFLICT(label) DO UPDATE SET marker=excluded.id RETURNING id,marker" } else { "INSERT INTO upsert_default_rows(label,marker) VALUES ('same',0) ON CONFLICT(label) DO UPDATE SET marker=excluded.id RETURNING id,marker" };
            let pending = tokio::spawn(async move { waiter.query_one(sql, &[]).await });
            wait_for_upsert_boundary(&server, &pending).await;
            // Act
            owner.batch_execute(if commit { "COMMIT" } else { "ROLLBACK" }).await.expect("resolve owner");
            let returned = tokio::time::timeout(Duration::from_secs(5), pending).await.expect("upsert completes").expect("waiter task").expect("upsert row");
            let next = owner.query_one("INSERT INTO upsert_default_rows(label,marker) VALUES ('next',0) RETURNING id", &[]).await.expect("next serial value");
            // Assert
            assert_eq!(returned.get::<_,i32>(0), if commit { 1 } else { 2 });
            assert_eq!(returned.get::<_,i32>(1), if commit { 2 } else { 0 });
            assert_eq!(next.get::<_,i32>(0),3,"wait must not reevaluate sequence defaults");
            assert_eq!(server.cassie.metrics()["transactions"]["unique_waiters"],0);
            drop(owner);
            server.shutdown(owner_connection).await;
            waiter_connection.abort();
            let _ = waiter_connection.await;
        }
    });
}

#[test]
fn should_release_waiting_upsert_candidates_given_cancellation_when_the_owner_still_holds_its_key()
{
    // Arrange
    current_thread_runtime().block_on(async {
        let server = CompatibilityServer::start("upsert_cancel").await;
        let (owner, owner_connection) = server.connect().await;
        let (waiter, waiter_connection) = server.connect().await;
        create_upsert_fixture(&owner).await;
        let cancellation = waiter.cancel_token();
        let pending = start_waiting_upsert(waiter);
        wait_for_upsert_boundary(&server, &pending).await;
        // Act
        cancellation
            .cancel_query(NoTls)
            .await
            .expect("cancel waiting statement");
        let cancelled = tokio::time::timeout(Duration::from_secs(5), pending)
            .await
            .expect("cancel completes")
            .expect("waiter task")
            .expect_err("cancelled upsert");
        owner
            .batch_execute("COMMIT")
            .await
            .expect("owner can still commit");
        let row = owner
            .query_one(
                "SELECT inserted_by,update_count FROM compat_upsert_wait_rows",
                &[],
            )
            .await
            .expect("committed owner row");
        // Assert
        assert_eq!(
            cancelled.as_db_error().expect("SQL error").code().code(),
            "57014"
        );
        assert_eq!(row.get::<_, String>(0), "owner");
        assert_eq!(row.get::<_, i32>(1), 0);
        assert_eq!(server.cassie.metrics()["transactions"]["unique_waiters"], 0);
        assert_eq!(
            server.cassie.metrics()["query"]["current_accounted_memory_bytes"],
            0
        );
        drop(owner);
        server.shutdown(owner_connection).await;
        waiter_connection.abort();
        let _ = waiter_connection.await;
    });
}

#[test]
fn should_release_unused_upsert_claims_given_a_false_action_filter_when_the_transaction_stays_open()
{
    // Arrange
    current_thread_runtime().block_on(async {
        let server = CompatibilityServer::start("upsert_filtered_claim").await;
        let (owner, owner_connection) = server.connect().await;
        let (waiter, waiter_connection) = server.connect().await;
        create_upsert_fixture(&owner).await;
        waiter.batch_execute("BEGIN").await.expect("explicit waiter transaction");
        let pending = tokio::spawn(async move {
            let result = waiter.query("INSERT INTO compat_upsert_wait_rows VALUES (1,'filtered',0) ON CONFLICT(id) DO UPDATE SET update_count=1 WHERE false RETURNING id", &[]).await;
            (waiter,result)
        });
        wait_for_upsert_boundary(&server, &pending).await;
        // Act
        owner.batch_execute("COMMIT").await.expect("resolve owner");
        let (waiter, result) = tokio::time::timeout(Duration::from_secs(5),pending).await.expect("filtered action completes").expect("waiter task");
        let following = tokio::time::timeout(Duration::from_secs(5),owner.query_one("INSERT INTO compat_upsert_wait_rows VALUES (1,'next',0) ON CONFLICT(id) DO UPDATE SET update_count=compat_upsert_wait_rows.update_count+1 RETURNING update_count", &[])).await.expect("unused claim must not block next writer").expect("following writer");
        // Assert
        assert!(result.expect("filtered action succeeds").is_empty());
        assert_eq!(following.get::<_,i32>(0),1);
        waiter.batch_execute("ROLLBACK").await.expect("finish waiter");
        drop((owner,waiter));
        server.shutdown(owner_connection).await;
        waiter_connection.abort();
        let _ = waiter_connection.await;
    });
}

#[test]
fn should_recheck_nonarbiter_uniqueness_given_owner_resolution_when_upsert_selects_one_constraint()
{
    // Arrange
    current_thread_runtime().block_on(async {
        for commit in [true,false] {
            let server=CompatibilityServer::start("upsert_nonarbiter").await;
            let (owner,owner_connection)=server.connect().await;
            let (waiter,waiter_connection)=server.connect().await;
            for sql in ["CREATE TABLE upsert_nonarbiter(id INT PRIMARY KEY,token TEXT UNIQUE,marker INT)","INSERT INTO upsert_nonarbiter VALUES(2,'taken',0)","BEGIN","INSERT INTO upsert_nonarbiter VALUES(1,'owner',0)"] { owner.batch_execute(sql).await.expect(sql); }
            let pending=tokio::spawn(async move { waiter.query_one("INSERT INTO upsert_nonarbiter VALUES(1,'taken',0) ON CONFLICT(id) DO UPDATE SET marker=upsert_nonarbiter.marker+1 RETURNING id,token,marker",&[]).await });
            wait_for_upsert_boundary(&server,&pending).await;
            // Act
            owner.batch_execute(if commit {"COMMIT"}else{"ROLLBACK"}).await.expect("resolve owner");
            let result=tokio::time::timeout(Duration::from_secs(5),pending).await.expect("upsert finishes").expect("waiter task");
            let rows=owner.query("SELECT id FROM upsert_nonarbiter ORDER BY id",&[]).await.expect("atomic final rows");
            // Assert
            if commit {
                let row=result.expect("update ignores unused conflicting proposed token");
                assert_eq!(row.get::<_,i32>(0),1);
                assert_eq!(row.get::<_,String>(1),"owner");
                assert_eq!(row.get::<_,i32>(2),1);
                assert_eq!(rows.len(),2);
            } else {
                assert_eq!(result.expect_err("insert checks other unique constraint").as_db_error().expect("SQL error").code().code(),"23505");
                assert_eq!(rows.len(),1);
                assert_eq!(rows[0].get::<_,i32>(0),2);
            }
            drop(owner);
            server.shutdown(owner_connection).await;
            waiter_connection.abort();let _=waiter_connection.await;
        }
    });
}

#[test]
fn should_retain_prior_transaction_keys_given_cancelled_upsert_when_rollback_has_not_occurred() {
    // Arrange
    current_thread_runtime().block_on(async {
        let server=CompatibilityServer::start("upsert_prior_claims").await;
        let (owner,owner_connection)=server.connect().await;
        let (waiter,waiter_connection)=server.connect().await;
        let (following,following_connection)=server.connect().await;
        create_upsert_fixture(&owner).await;
        waiter.batch_execute("BEGIN").await.expect("begin waiter");
        waiter.batch_execute("INSERT INTO compat_upsert_wait_rows VALUES(7,'prior',0)").await.expect("own earlier key");
        let cancellation=waiter.cancel_token();
        let pending=tokio::spawn(async move {
            let result=waiter.query_one("INSERT INTO compat_upsert_wait_rows VALUES(1,'waiter',0) ON CONFLICT(id) DO UPDATE SET update_count=1 RETURNING id",&[]).await;
            (waiter,result)
        });
        wait_for_upsert_boundary(&server,&pending).await;
        // Act
        cancellation.cancel_query(NoTls).await.expect("cancel wait");
        let (waiter,cancelled)=tokio::time::timeout(Duration::from_secs(5),pending).await.expect("cancel finishes").expect("waiter task");
        let following=start_waiting_key_seven(following);
        wait_for_upsert_boundary(&server,&following).await;
        owner.batch_execute("COMMIT").await.expect("owner key unaffected");
        waiter.batch_execute("ROLLBACK").await.expect("release earlier key");
        let row=tokio::time::timeout(Duration::from_secs(5),following).await.expect("rollback releases prior key").expect("following task").expect("following insert");
        // Assert
        assert_eq!(cancelled.expect_err("cancelled statement").as_db_error().expect("SQL error").code().code(),"57014");
        assert_eq!(row.get::<_,String>(0),"following");
        assert_eq!(server.cassie.metrics()["transactions"]["unique_waiters"],0);
        drop((owner,waiter));server.shutdown(owner_connection).await;
        waiter_connection.abort();following_connection.abort();
        let _=waiter_connection.await;let _=following_connection.await;
    });
}

fn start_waiting_key_seven(
    client: tokio_postgres::Client,
) -> tokio::task::JoinHandle<Result<tokio_postgres::Row, tokio_postgres::Error>> {
    tokio::spawn(async move {
        client.query_one("INSERT INTO compat_upsert_wait_rows VALUES(7,'following',0) ON CONFLICT(id) DO UPDATE SET update_count=1 RETURNING inserted_by",&[]).await
    })
}

#[test]
fn should_publish_no_partial_upsert_given_repeated_conflict_rows_when_the_owner_commits() {
    // Arrange
    current_thread_runtime().block_on(async {
        let server=CompatibilityServer::start("upsert_atomic_cardinality").await;
        let (owner,owner_connection)=server.connect().await;
        let (waiter,waiter_connection)=server.connect().await;
        create_upsert_fixture(&owner).await;
        let pending=tokio::spawn(async move { waiter.query("INSERT INTO compat_upsert_wait_rows VALUES(1,'first',0),(1,'second',0) ON CONFLICT(id) DO UPDATE SET update_count=compat_upsert_wait_rows.update_count+1 RETURNING id",&[]).await });
        wait_for_upsert_boundary(&server,&pending).await;
        // Act
        owner.batch_execute("COMMIT").await.expect("resolve owner");
        let error=tokio::time::timeout(Duration::from_secs(5),pending).await.expect("statement finishes").expect("waiter task").expect_err("one row affected twice");
        let row=owner.query_one("SELECT inserted_by,update_count FROM compat_upsert_wait_rows WHERE id=1",&[]).await.expect("committed state");
        let next=tokio::time::timeout(Duration::from_secs(5),owner.query_one("INSERT INTO compat_upsert_wait_rows VALUES(1,'next',0) ON CONFLICT(id) DO UPDATE SET update_count=1 RETURNING update_count",&[])).await.expect("failed statement released claims").expect("next upsert");
        // Assert
        assert_eq!(error.as_db_error().expect("SQL error").code().code(),"21000");
        assert_eq!(row.get::<_,String>(0),"owner");
        assert_eq!(row.get::<_,i32>(1),0);
        assert_eq!(next.get::<_,i32>(0),1);
        drop(owner);server.shutdown(owner_connection).await;
        waiter_connection.abort();let _=waiter_connection.await;
    });
}

#[test]
fn should_preserve_json_sql_null_given_owner_resolution_when_upsert_has_a_column_default() {
    // Arrange
    current_thread_runtime().block_on(async {
        for commit in [true, false] {
            let server = CompatibilityServer::start("upsert_json_null_default").await;
            let (owner, owner_connection) = server.connect().await;
            let (waiter, waiter_connection) = server.connect().await;
            for sql in [
                "CREATE TABLE upsert_json_null_rows (id INT PRIMARY KEY, v JSONB DEFAULT '{\"default\":1}'::jsonb)",
                "BEGIN",
                "INSERT INTO upsert_json_null_rows VALUES (1,'{\"owner\":1}'::jsonb)",
            ] {
                owner.batch_execute(sql).await.expect("stage JSON owner");
            }
            let pending = tokio::spawn(async move {
                waiter.query_one("INSERT INTO upsert_json_null_rows VALUES (1,NULL) ON CONFLICT(id) DO UPDATE SET v=excluded.v WHERE excluded.v IS NULL RETURNING v IS NULL", &[]).await
            });
            wait_for_upsert_boundary(&server, &pending).await;

            // Act
            owner.batch_execute(if commit { "COMMIT" } else { "ROLLBACK" })
                .await.expect("resolve JSON owner");
            let returned = tokio::time::timeout(Duration::from_secs(5), pending)
                .await.expect("waiter completes").expect("waiter task").expect("upsert row");
            let stored = owner.query_one("SELECT v IS NULL FROM upsert_json_null_rows", &[])
                .await.expect("read final JSON value");

            // Assert
            assert!(returned.get::<_, bool>(0));
            assert!(stored.get::<_, bool>(0));
            assert_eq!(server.cassie.metrics()["query"]["current_accounted_memory_bytes"], 0);
            drop(owner);
            server.shutdown(owner_connection).await;
            waiter_connection.abort();
            let _ = waiter_connection.await;
        }
    });
}
