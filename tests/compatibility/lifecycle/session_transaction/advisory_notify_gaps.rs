use super::*;

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
