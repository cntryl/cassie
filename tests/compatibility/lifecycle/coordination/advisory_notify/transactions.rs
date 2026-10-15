struct UnlockAllObservations {
    unlock_all_oid: u32,
    initially_blocked: [bool; 3],
    released_after_unlock_all: [bool; 2],
    contender_unlock_succeeded: [bool; 2],
    transaction_still_blocked: bool,
    transaction_released_after_rollback: bool,
}

async fn exercise_unlock_all_ownership(
    owner: &tokio_postgres::Client,
    contender: &tokio_postgres::Client,
) -> UnlockAllObservations {
    owner
        .batch_execute("BEGIN")
        .await
        .expect("begin owner transaction");
    owner
        .query_one("SELECT pg_advisory_lock(91822022)", &[])
        .await
        .expect("acquire one-key session lock");
    owner
        .query_one("SELECT pg_advisory_lock(71, 31)", &[])
        .await
        .expect("acquire two-key session lock");
    owner
        .query_one("SELECT pg_advisory_xact_lock(91822023)", &[])
        .await
        .expect("acquire transaction lock");
    contender
        .batch_execute("BEGIN")
        .await
        .expect("begin contender transaction");
    let initially_blocked = [
        contender
            .query_one("SELECT pg_try_advisory_lock(91822022)", &[])
            .await
            .expect("observe one-key lock")
            .get(0),
        contender
            .query_one("SELECT pg_try_advisory_lock_shared(71, 31)", &[])
            .await
            .expect("observe two-key lock")
            .get(0),
        contender
            .query_one("SELECT pg_try_advisory_xact_lock(91822023)", &[])
            .await
            .expect("observe transaction lock")
            .get(0),
    ];
    let unlock_all = owner
        .query_one("SELECT pg_advisory_unlock_all()", &[])
        .await
        .expect("release session locks");
    let released_after_unlock_all = [
        contender
            .query_one("SELECT pg_try_advisory_lock(91822022)", &[])
            .await
            .expect("acquire one-key lock after unlock-all")
            .get(0),
        contender
            .query_one("SELECT pg_try_advisory_lock_shared(71, 31)", &[])
            .await
            .expect("acquire two-key lock after unlock-all")
            .get(0),
    ];
    let contender_unlock_succeeded = [
        contender
            .query_one("SELECT pg_advisory_unlock(91822022)", &[])
            .await
            .expect("release contender one-key lock")
            .get(0),
        contender
            .query_one("SELECT pg_advisory_unlock_shared(71, 31)", &[])
            .await
            .expect("release contender two-key lock")
            .get(0),
    ];
    let transaction_still_blocked = contender
        .query_one("SELECT pg_try_advisory_xact_lock(91822023)", &[])
        .await
        .expect("verify transaction lock remains held")
        .get(0);
    owner
        .batch_execute("ROLLBACK")
        .await
        .expect("release transaction lock");
    let transaction_released_after_rollback = contender
        .query_one("SELECT pg_try_advisory_xact_lock(91822023)", &[])
        .await
        .expect("acquire transaction lock after rollback")
        .get(0);
    contender
        .batch_execute("COMMIT")
        .await
        .expect("release contender transaction lock");
    UnlockAllObservations {
        unlock_all_oid: unlock_all.columns()[0].type_().oid(),
        initially_blocked,
        released_after_unlock_all,
        contender_unlock_succeeded,
        transaction_still_blocked,
        transaction_released_after_rollback,
    }
}

#[test]
fn should_release_session_locks_but_keep_transaction_locks_with_unlock_all() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("advisory_unlock_all_ownership").await;
        let (owner, owner_connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        // Act
        let observations = exercise_unlock_all_ownership(&owner, &contender).await;

        // Assert
        assert_eq!(observations.unlock_all_oid, 2278);
        assert_eq!(observations.initially_blocked, [false, false, false]);
        assert_eq!(observations.released_after_unlock_all, [true, true]);
        assert_eq!(observations.contender_unlock_succeeded, [true, true]);
        assert!(!observations.transaction_still_blocked);
        assert!(observations.transaction_released_after_rollback);

        drop((owner, contender));
        server.shutdown(owner_connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
    });
}
#[test]
fn should_try_single_key_shared_transaction_advisory_lock() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("try_shared_xact_lock_single").await;
        let (client, connection) = server.connect().await;
        let statement = client
            .prepare("SELECT pg_try_advisory_xact_lock_shared(CAST($1 AS BIGINT))")
            .await
            .expect("prepare single-key shared transaction lock probe");

        // Act
        let acquired = client
            .query_one(&statement, &[&9_011_225_i64])
            .await
            .expect("try the single-key shared transaction lock");

        // Assert
        assert_eq!(statement.columns()[0].type_().oid(), 16);
        assert!(acquired.get::<_, bool>(0));

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_try_two_key_shared_transaction_advisory_lock() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("try_shared_xact_lock_two_key").await;
        let (client, connection) = server.connect().await;
        let statement = client
            .prepare("SELECT pg_try_advisory_xact_lock_shared(71, 29)")
            .await
            .expect("prepare two-key shared transaction lock probe");

        // Act
        let acquired = client
            .query_one(&statement, &[])
            .await
            .expect("try the two-key shared transaction lock");

        // Assert
        assert_eq!(statement.columns()[0].type_().oid(), 16);
        assert!(acquired.get::<_, bool>(0));

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_release_two_key_transaction_advisory_lock_on_rollback() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("two_key_xact_lock_rollback").await;
        let (owner, owner_connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        owner
            .batch_execute("BEGIN; SELECT pg_advisory_xact_lock(71, 30)")
            .await
            .expect("hold a two-key transaction advisory lock");
        contender
            .batch_execute("BEGIN")
            .await
            .expect("begin the lock contender transaction");
        let blocked: bool = contender
            .query_one("SELECT pg_try_advisory_xact_lock(71, 30)", &[])
            .await
            .expect("observe the owner transaction lock")
            .get(0);

        // Act
        owner
            .batch_execute("ROLLBACK")
            .await
            .expect("release the owner transaction lock by rolling back");
        let acquired_after_rollback: bool = contender
            .query_one("SELECT pg_try_advisory_xact_lock(71, 30)", &[])
            .await
            .expect("acquire the lock after owner rollback")
            .get(0);
        contender
            .batch_execute("COMMIT")
            .await
            .expect("release the contender transaction lock");

        // Assert
        assert!(!blocked);
        assert!(acquired_after_rollback);

        drop((owner, contender));
        server.shutdown(owner_connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
    });
}
#[test]
fn should_track_transactional_listen_state() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("transactional_listen_state").await;
        let (client, connection) = server.connect().await;

        // Act
        client
            .batch_execute("BEGIN; LISTEN compat_transactional_events")
            .await
            .expect("stage LISTEN inside a transaction");
        let before_listen_commit: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_listening_channels() \
                 WHERE pg_listening_channels = 'compat_transactional_events'",
                &[],
            )
            .await
            .expect("inspect listener state before commit")
            .get(0);
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the staged LISTEN");
        let after_listen_rollback: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_listening_channels() \
                 WHERE pg_listening_channels = 'compat_transactional_events'",
                &[],
            )
            .await
            .expect("inspect listener state after rollback")
            .get(0);
        client
            .batch_execute("BEGIN; LISTEN compat_transactional_events; COMMIT")
            .await
            .expect("commit LISTEN for the session");
        let after_listen_commit: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_listening_channels() \
                 WHERE pg_listening_channels = 'compat_transactional_events'",
                &[],
            )
            .await
            .expect("inspect the committed listener")
            .get(0);
        client
            .batch_execute("BEGIN; UNLISTEN compat_transactional_events")
            .await
            .expect("stage UNLISTEN inside a transaction");
        let before_unlisten_commit: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_listening_channels() \
                 WHERE pg_listening_channels = 'compat_transactional_events'",
                &[],
            )
            .await
            .expect("inspect listener state before UNLISTEN commit")
            .get(0);
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the staged UNLISTEN");
        let after_unlisten_rollback: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_listening_channels() \
                 WHERE pg_listening_channels = 'compat_transactional_events'",
                &[],
            )
            .await
            .expect("inspect listener state after UNLISTEN rollback")
            .get(0);
        client
            .batch_execute("UNLISTEN compat_transactional_events")
            .await
            .expect("commit UNLISTEN outside an explicit transaction");
        let after_unlisten_commit: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_listening_channels() \
                 WHERE pg_listening_channels = 'compat_transactional_events'",
                &[],
            )
            .await
            .expect("inspect listener state after UNLISTEN commit")
            .get(0);

        // Assert
        assert_eq!(before_listen_commit, 0);
        assert_eq!(after_listen_rollback, 0);
        assert_eq!(after_listen_commit, 1);
        assert_eq!(before_unlisten_commit, 1);
        assert_eq!(after_unlisten_rollback, 1);
        assert_eq!(after_unlisten_commit, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
