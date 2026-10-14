use super::*;

mod advisory_lock_key_space_identity;
mod notification_deduplication_key;

#[test]
fn should_return_float8_notification_queue_usage() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("notification_queue_usage").await;
        let (client, connection) = server.connect().await;
        let statement = client
            .prepare("SELECT pg_notification_queue_usage()")
            .await
            .expect("prepare notification queue usage query");

        // Act
        let usage = client
            .query_one(&statement, &[])
            .await
            .expect("read notification queue usage");

        // Assert
        assert_eq!(statement.columns()[0].type_().oid(), 701);
        assert_eq!(usage.get::<_, f64>(0), 0.0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_accept_notify_without_a_listening_session() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("notify_without_listener").await;
        let (client, connection) = server.connect().await;

        // Act
        client
            .batch_execute("NOTIFY compat_no_listener, 'payload-822'")
            .await
            .expect("NOTIFY is valid when no session is listening");
        let value: i32 = client
            .query_one("SELECT 1", &[])
            .await
            .expect("session remains usable after NOTIFY")
            .get(0);

        // Assert
        assert_eq!(value, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_accept_unlisten_for_an_unsubscribed_channel() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("unlisten_named_channel").await;
        let (client, connection) = server.connect().await;

        // Act
        client
            .batch_execute("UNLISTEN compat_never_listened")
            .await
            .expect("UNLISTEN of an unregistered channel is valid");
        let value: i32 = client
            .query_one("SELECT 1", &[])
            .await
            .expect("session remains usable after UNLISTEN")
            .get(0);

        // Assert
        assert_eq!(value, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_return_void_from_advisory_unlock_all() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("advisory_unlock_all").await;
        let (client, connection) = server.connect().await;

        // Act
        let statement = client
            .prepare("SELECT pg_advisory_unlock_all()")
            .await
            .expect("prepare the advisory unlock-all function");
        let row = client
            .query_one("SELECT pg_advisory_unlock_all()::TEXT", &[])
            .await
            .expect("pg_advisory_unlock_all returns PostgreSQL VOID");

        // Assert
        assert_eq!(statement.columns()[0].type_().oid(), 2278);
        assert_eq!(row.get::<_, String>(0), "");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_release_session_locks_but_keep_transaction_locks_with_unlock_all() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("advisory_unlock_all_ownership").await;
        let (owner, owner_connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin the transaction holding both lock kinds");
        owner
            .query_one("SELECT pg_advisory_lock(91822022)", &[])
            .await
            .expect("acquire a one-key session lock");
        owner
            .query_one("SELECT pg_advisory_lock(71, 31)", &[])
            .await
            .expect("acquire a two-key session lock");
        owner
            .query_one("SELECT pg_advisory_xact_lock(91822023)", &[])
            .await
            .expect("acquire a transaction lock");
        contender
            .batch_execute("BEGIN")
            .await
            .expect("begin the lock contender transaction");
        let one_key_blocked: bool = contender
            .query_one("SELECT pg_try_advisory_lock(91822022)", &[])
            .await
            .expect("observe the held one-key session lock")
            .get(0);
        let two_key_blocked: bool = contender
            .query_one("SELECT pg_try_advisory_lock_shared(71, 31)", &[])
            .await
            .expect("observe the held two-key session lock")
            .get(0);
        let transaction_blocked: bool = contender
            .query_one("SELECT pg_try_advisory_xact_lock(91822023)", &[])
            .await
            .expect("observe the owner transaction lock")
            .get(0);

        // Act
        let unlock_all = owner
            .query_one("SELECT pg_advisory_unlock_all()", &[])
            .await
            .expect("release all session-level advisory locks");
        let one_key_released: bool = contender
            .query_one("SELECT pg_try_advisory_lock(91822022)", &[])
            .await
            .expect("acquire the one-key lock after unlock-all")
            .get(0);
        let one_key_cleanup: bool = contender
            .query_one("SELECT pg_advisory_unlock(91822022)", &[])
            .await
            .expect("release the contender one-key lock")
            .get(0);
        let two_key_released: bool = contender
            .query_one("SELECT pg_try_advisory_lock_shared(71, 31)", &[])
            .await
            .expect("acquire the two-key shared lock after unlock-all")
            .get(0);
        let two_key_cleanup: bool = contender
            .query_one("SELECT pg_advisory_unlock_shared(71, 31)", &[])
            .await
            .expect("release the contender two-key shared lock")
            .get(0);
        let transaction_still_blocked: bool = contender
            .query_one("SELECT pg_try_advisory_xact_lock(91822023)", &[])
            .await
            .expect("verify unlock-all preserves transaction-level locks")
            .get(0);
        owner
            .batch_execute("ROLLBACK")
            .await
            .expect("release the transaction-level lock by rollback");
        let transaction_released: bool = contender
            .query_one("SELECT pg_try_advisory_xact_lock(91822023)", &[])
            .await
            .expect("acquire the transaction lock after owner rollback")
            .get(0);
        contender
            .batch_execute("COMMIT")
            .await
            .expect("release the contender transaction lock");

        // Assert
        assert_eq!(unlock_all.columns()[0].type_().oid(), 2278);
        assert!(!one_key_blocked);
        assert!(!two_key_blocked);
        assert!(!transaction_blocked);
        assert!(one_key_released);
        assert!(one_key_cleanup);
        assert!(two_key_released);
        assert!(two_key_cleanup);
        assert!(!transaction_still_blocked);
        assert!(transaction_released);

        drop((owner, contender));
        server.shutdown(owner_connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
    });
}

#[test]
fn should_return_false_from_two_key_advisory_unlock_when_unowned() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("advisory_unlock_two_key").await;
        let (client, connection) = server.connect().await;

        // Act
        let released: bool = client
            .query_one("SELECT pg_advisory_unlock(71, 29)", &[])
            .await
            .expect("call the two-key exclusive unlock overload")
            .get(0);

        // Assert
        assert!(!released, "an unowned two-key lock cannot be released");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_return_false_from_two_key_shared_advisory_unlock_when_unowned() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("advisory_unlock_shared_two_key").await;
        let (client, connection) = server.connect().await;

        // Act
        let released: bool = client
            .query_one("SELECT pg_advisory_unlock_shared(71, 29)", &[])
            .await
            .expect("call the two-key shared unlock overload")
            .get(0);

        // Assert
        assert!(
            !released,
            "an unowned two-key shared lock cannot be released"
        );

        drop(client);
        server.shutdown(connection).await;
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
fn should_release_reentrant_session_advisory_lock_one_acquisition_at_a_time() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("advisory_lock_reentrancy").await;
        let (owner, owner_connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        owner
            .query_one("SELECT pg_advisory_lock(91822019)", &[])
            .await
            .expect("acquire the first session-lock reference");
        owner
            .query_one("SELECT pg_advisory_lock(91822019)", &[])
            .await
            .expect("acquire the same session lock reentrantly");

        // Act
        let blocked_twice: bool = contender
            .query_one("SELECT pg_try_advisory_lock(91822019)", &[])
            .await
            .expect("probe the twice-acquired session lock")
            .get(0);
        let first_release: bool = owner
            .query_one("SELECT pg_advisory_unlock(91822019)", &[])
            .await
            .expect("release one reentrant lock reference")
            .get(0);
        let blocked_once: bool = contender
            .query_one("SELECT pg_try_advisory_lock(91822019)", &[])
            .await
            .expect("probe the remaining session-lock reference")
            .get(0);
        let second_release: bool = owner
            .query_one("SELECT pg_advisory_unlock(91822019)", &[])
            .await
            .expect("release the final reentrant lock reference")
            .get(0);
        let acquired_after_release: bool = contender
            .query_one("SELECT pg_try_advisory_lock(91822019)", &[])
            .await
            .expect("acquire the lock after its owner released both references")
            .get(0);
        let contender_release: bool = contender
            .query_one("SELECT pg_advisory_unlock(91822019)", &[])
            .await
            .expect("release the contender session lock")
            .get(0);

        // Assert
        assert!(!blocked_twice);
        assert!(first_release);
        assert!(!blocked_once);
        assert!(second_release);
        assert!(acquired_after_release);
        assert!(contender_release);

        drop((owner, contender));
        server.shutdown(owner_connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
    });
}

#[test]
fn should_wait_for_session_advisory_lock_until_owner_releases_it() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("blocking_session_advisory_lock").await;
        let (owner, owner_connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        owner
            .query_one("SELECT pg_advisory_lock(91822020)", &[])
            .await
            .expect("acquire the session advisory lock");

        // Act
        let mut acquisition = tokio::spawn(async move {
            let result = contender
                .query_one("SELECT pg_advisory_lock(91822020)", &[])
                .await;
            (contender, result)
        });
        let before_release =
            tokio::time::timeout(Duration::from_millis(100), &mut acquisition).await;
        let returned_before_release = before_release.is_ok();
        let owner_released: bool = owner
            .query_one("SELECT pg_advisory_unlock(91822020)", &[])
            .await
            .expect("release the blocking session lock")
            .get(0);
        let (contender, acquired) = match before_release {
            Ok(joined) => joined.expect("contender task should complete"),
            Err(_) => tokio::time::timeout(Duration::from_secs(2), &mut acquisition)
                .await
                .expect("blocked advisory lock should resume after owner release")
                .expect("contender task should complete after owner release"),
        };
        let acquired_successfully = acquired.is_ok();
        let contender_released = if acquired_successfully {
            contender
                .query_one("SELECT pg_advisory_unlock(91822020)", &[])
                .await
                .and_then(|row| row.try_get::<_, bool>(0))
                .unwrap_or(false)
        } else {
            false
        };

        // Assert
        assert!(!returned_before_release);
        assert!(owner_released);
        assert!(acquired_successfully);
        assert!(contender_released);

        drop((owner, contender));
        server.shutdown(owner_connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
    });
}

#[test]
fn should_cancel_blocking_advisory_lock_wait_at_statement_timeout() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("advisory_lock_wait_cancellation").await;
        let (owner, owner_connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        owner
            .query_one("SELECT pg_advisory_lock(91822021)", &[])
            .await
            .expect("hold the session advisory lock");
        contender
            .batch_execute("SET statement_timeout = '50ms'")
            .await
            .expect("set a deadline for the blocking lock request");

        // Act
        let mut lock_wait = tokio::spawn(async move {
            let result = contender
                .query_one("SELECT pg_advisory_lock(91822021)", &[])
                .await;
            (contender, result)
        });
        let bounded_wait = tokio::time::timeout(Duration::from_secs(2), &mut lock_wait).await;
        let owner_released_for_cleanup = bounded_wait.is_err();
        if owner_released_for_cleanup {
            owner
                .query_one("SELECT pg_advisory_unlock(91822021)", &[])
                .await
                .expect("release the owner lock if statement timeout did not fire");
        }
        let (contender, cancelled) = match bounded_wait {
            Ok(joined) => joined.expect("lock wait task should complete"),
            Err(_) => tokio::time::timeout(Duration::from_secs(2), &mut lock_wait)
                .await
                .expect("lock wait should complete after owner releases the lock")
                .expect("lock wait task should complete after owner release"),
        };
        let cancellation_sqlstate = cancelled
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_string());
        let acquired_during_cleanup = if cancelled.is_ok() {
            contender
                .query_one("SELECT pg_advisory_unlock(91822021)", &[])
                .await
                .and_then(|row| row.try_get::<_, bool>(0))
                .unwrap_or(false)
        } else {
            false
        };
        contender
            .batch_execute("RESET statement_timeout")
            .await
            .expect("restore the contender session timeout");
        let recovered: i32 = contender
            .query_one("SELECT 1", &[])
            .await
            .expect("reuse the contender session after cancellation")
            .get(0);
        if !owner_released_for_cleanup {
            owner
                .query_one("SELECT pg_advisory_unlock(91822021)", &[])
                .await
                .expect("release the owner lock after the cancellation result");
        }
        let acquired_after_release: bool = contender
            .query_one("SELECT pg_try_advisory_lock(91822021)", &[])
            .await
            .expect("acquire the released advisory lock")
            .get(0);
        let contender_released: bool = contender
            .query_one("SELECT pg_advisory_unlock(91822021)", &[])
            .await
            .expect("release the contender session lock")
            .get(0);

        // Assert
        assert_eq!(cancellation_sqlstate.as_deref(), Some("57014"));
        assert!(!acquired_during_cleanup);
        assert_eq!(recovered, 1);
        assert!(acquired_after_release);
        assert!(contender_released);

        drop((owner, contender));
        server.shutdown(owner_connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
    });
}

#[test]
fn should_allow_two_key_shared_locks_but_exclude_an_exclusive_lock() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("two_key_shared_session_locks").await;
        let (owner, owner_connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        owner
            .query_one("SELECT pg_advisory_lock_shared(71, 30)", &[])
            .await
            .expect("acquire a two-key shared session lock");

        // Act
        let shared_acquired: bool = contender
            .query_one("SELECT pg_try_advisory_lock_shared(71, 30)", &[])
            .await
            .expect("acquire a compatible two-key shared session lock")
            .get(0);
        let contender_shared_released: bool = contender
            .query_one("SELECT pg_advisory_unlock_shared(71, 30)", &[])
            .await
            .expect("release the contender shared lock")
            .get(0);
        let exclusive_blocked: bool = contender
            .query_one("SELECT pg_try_advisory_lock(71, 30)", &[])
            .await
            .expect("probe exclusive acquisition while the owner holds a shared lock")
            .get(0);
        let owner_shared_released: bool = owner
            .query_one("SELECT pg_advisory_unlock_shared(71, 30)", &[])
            .await
            .expect("release the owner shared lock")
            .get(0);
        let exclusive_acquired: bool = contender
            .query_one("SELECT pg_try_advisory_lock(71, 30)", &[])
            .await
            .expect("acquire an exclusive lock after shared owners release")
            .get(0);
        let exclusive_released: bool = contender
            .query_one("SELECT pg_advisory_unlock(71, 30)", &[])
            .await
            .expect("release the two-key exclusive session lock")
            .get(0);

        // Assert
        assert!(shared_acquired);
        assert!(contender_shared_released);
        assert!(!exclusive_blocked);
        assert!(owner_shared_released);
        assert!(exclusive_acquired);
        assert!(exclusive_released);

        drop((owner, contender));
        server.shutdown(owner_connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
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

#[test]
fn should_reject_oversized_notification_payload_with_sqlstate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("notify_payload_limit").await;
        let (client, connection) = server.connect().await;

        // Act
        let boundary = client
            .query_one(
                "SELECT pg_notify('compat_payload_limit', repeat('x', 7999))",
                &[],
            )
            .await
            .expect("accept a notification payload of 7999 bytes");
        let too_long = client
            .query_one(
                "SELECT pg_notify('compat_payload_limit', repeat('x', 8000))",
                &[],
            )
            .await
            .expect_err("reject a notification payload of 8000 bytes");
        let recovered: i32 = client
            .query_one("SELECT 1", &[])
            .await
            .expect("reuse the session after payload rejection")
            .get(0);

        // Assert
        assert_eq!(boundary.columns()[0].type_().oid(), 2278);
        assert_eq!(db_error(&too_long).code().code(), "22023");
        assert_eq!(recovered, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
