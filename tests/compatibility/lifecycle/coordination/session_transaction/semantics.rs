#[test]
fn should_keep_try_advisory_lock_false_until_the_other_session_commits() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("advisory_lock_ownership").await;
        let (owner, owner_connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        execute_statements(&owner, &["BEGIN", "SELECT pg_advisory_xact_lock(91822001)"])
            .await
            .expect("owner should acquire a transaction advisory lock");

        // Act
        let busy: bool = contender
            .query_one("SELECT pg_try_advisory_xact_lock(91822001)", &[])
            .await
            .expect("try lock from independent session")
            .try_get(0)
            .expect("try-lock result");
        owner
            .batch_execute("COMMIT")
            .await
            .expect("release owner lock");
        let available: bool = contender
            .query_one("SELECT pg_try_advisory_xact_lock(91822001)", &[])
            .await
            .expect("retry lock after owner commit")
            .try_get(0)
            .expect("try-lock result");

        // Assert
        assert!(!busy, "a held advisory lock cannot be a successful no-op");
        assert!(available, "commit must release the transaction lock");

        drop((owner, contender));
        server.shutdown(owner_connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
    });
}
#[test]
fn should_keep_session_advisory_locks_until_explicit_unlock() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("session_advisory_unlock").await;
        let (owner, owner_connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        owner
            .query_one("SELECT pg_advisory_lock(91822003)", &[])
            .await
            .expect("owner should acquire a session advisory lock");

        // Act
        let blocked: bool = contender
            .query_one("SELECT pg_try_advisory_lock(91822003)", &[])
            .await
            .expect("try the lock from a second session")
            .try_get(0)
            .expect("try-lock result");
        let released: bool = owner
            .query_one("SELECT pg_advisory_unlock(91822003)", &[])
            .await
            .expect("release the session lock explicitly")
            .try_get(0)
            .expect("unlock result");
        let available: bool = contender
            .query_one("SELECT pg_try_advisory_lock(91822003)", &[])
            .await
            .expect("retry the lock after explicit unlock")
            .try_get(0)
            .expect("try-lock result");
        let contender_released: bool = contender
            .query_one("SELECT pg_advisory_unlock(91822003)", &[])
            .await
            .expect("release the contender's acquired lock")
            .try_get(0)
            .expect("unlock result");

        // Assert
        assert!(!blocked, "a held session lock must block another session");
        assert!(
            released,
            "the owning session should report a successful unlock"
        );
        assert!(available, "another session should acquire after unlock");
        assert!(contender_released);

        drop((owner, contender));
        server.shutdown(owner_connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
    });
}
#[test]
fn should_deliver_listen_notifications_to_the_subscribed_connection() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("listen_notify_delivery").await;
        let mut config = tokio_postgres::Config::new();
        config
            .host("127.0.0.1")
            .port(server.addr.port())
            .user("root")
            .password("postgres")
            .dbname("postgres");
        let (listener, mut listener_connection) = config
            .connect(NoTls)
            .await
            .expect("connect dedicated notification listener");
        let notifications = tokio::spawn(async move {
            loop {
                match std::future::poll_fn(|context| listener_connection.poll_message(context))
                    .await
                {
                    Some(Ok(tokio_postgres::AsyncMessage::Notification(notification))) => {
                        return Some((
                            notification.channel().to_string(),
                            notification.payload().to_string(),
                        ));
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) | None => return None,
                }
            }
        });
        listener
            .batch_execute("LISTEN compat_event_channel")
            .await
            .expect("subscribe before notification");
        let (sender, sender_connection) = server.connect().await;

        // Act
        sender
            .batch_execute("NOTIFY compat_event_channel, 'payload-42'")
            .await
            .expect("publish notification");
        let delivered = tokio::time::timeout(Duration::from_secs(2), notifications)
            .await
            .expect("notification should arrive within the bound")
            .expect("notification receiver task should finish");

        // Assert
        assert_eq!(
            delivered,
            Some(("compat_event_channel".to_string(), "payload-42".to_string()))
        );

        drop((listener, sender));
        server.shutdown(sender_connection).await;
    });
}
#[test]
fn should_block_exclusive_advisory_locks_while_shared_locks_are_held() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("shared_advisory_lock_compatibility").await;
        let (shared_owner, owner_connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        shared_owner
            .batch_execute("SELECT pg_advisory_lock_shared(9011223)")
            .await
            .expect("acquire a shared session advisory lock");

        // Act
        let exclusive_while_shared: bool = contender
            .query_one("SELECT pg_try_advisory_lock(9011223)", &[])
            .await
            .expect("try to acquire an exclusive lock held shared elsewhere")
            .try_get(0)
            .expect("exclusive lock result");
        let second_shared: bool = contender
            .query_one("SELECT pg_try_advisory_lock_shared(9011223)", &[])
            .await
            .expect("try to acquire a compatible shared lock")
            .try_get(0)
            .expect("shared lock result");
        let contender_unlocked: bool = contender
            .query_one("SELECT pg_advisory_unlock_shared(9011223)", &[])
            .await
            .expect("release the contender's shared lock")
            .try_get(0)
            .expect("shared unlock result");
        let owner_unlocked: bool = shared_owner
            .query_one("SELECT pg_advisory_unlock_shared(9011223)", &[])
            .await
            .expect("release the original shared lock")
            .try_get(0)
            .expect("owner unlock result");
        let exclusive_after_release: bool = contender
            .query_one("SELECT pg_try_advisory_lock(9011223)", &[])
            .await
            .expect("acquire exclusive lock after shared owners release")
            .try_get(0)
            .expect("exclusive lock after release");
        let exclusive_unlocked: bool = contender
            .query_one("SELECT pg_advisory_unlock(9011223)", &[])
            .await
            .expect("release the exclusive session lock")
            .try_get(0)
            .expect("exclusive unlock result");

        // Assert
        assert!(!exclusive_while_shared);
        assert!(second_shared);
        assert!(contender_unlocked);
        assert!(owner_unlocked);
        assert!(exclusive_after_release);
        assert!(exclusive_unlocked);

        drop(shared_owner);
        drop(contender);
        server.shutdown(owner_connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
    });
}
#[test]
fn should_accept_unlisten_all_when_the_session_has_no_subscriptions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("unlisten_all_without_channels").await;
        let (client, connection) = server.connect().await;

        // Act
        client
            .batch_execute("UNLISTEN *")
            .await
            .expect("UNLISTEN all is valid when no channels are registered");
        let value: i32 = client
            .query_one("SELECT 1", &[])
            .await
            .expect("the session should remain usable after UNLISTEN")
            .try_get(0)
            .expect("probe value");

        // Assert
        assert_eq!(value, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_return_no_listening_channels_for_an_unsubscribed_session() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("listening_channel_catalog").await;
        let (client, connection) = server.connect().await;

        // Act
        let channels = client
            .query("SELECT pg_listening_channels() ORDER BY 1", &[])
            .await;

        // Assert
        let channels = channels.expect("query pg_listening_channels for this session");
        assert!(channels.is_empty());

        drop(client);
        server.shutdown(connection).await;
    });
}
