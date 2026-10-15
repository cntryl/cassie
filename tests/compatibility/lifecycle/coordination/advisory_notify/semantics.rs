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
