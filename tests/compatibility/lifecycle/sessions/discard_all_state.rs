use super::*;

fn assert_discard_all_state(observations: &DiscardAllObservations) {
    assert!(!observations.advisory_locks.could_acquire_before_reset);
    assert_eq!(observations.application_name, "");
    assert_eq!(observations.cursor_count, 0);
    assert_eq!(observations.listening_channel_count, 0);
    assert!(observations.temporary_relation_is_missing);
    assert_eq!(
        db_error(
            observations
                .prepared_statement
                .as_ref()
                .expect_err("DISCARD ALL deallocates SQL PREPARE state")
        )
        .code()
        .code(),
        "26000"
    );
    assert_eq!(
        db_error(
            observations
                .cursor_fetch
                .as_ref()
                .expect_err("DISCARD ALL closes open cursors")
        )
        .code()
        .code(),
        "34000"
    );
    assert!(observations.advisory_locks.could_acquire_after_reset);
    assert!(observations.advisory_locks.released_after_reset);
}

struct AdvisoryLockObservations {
    could_acquire_before_reset: bool,
    could_acquire_after_reset: bool,
    released_after_reset: bool,
}

struct DiscardAllObservations {
    advisory_locks: AdvisoryLockObservations,
    application_name: String,
    cursor_count: i64,
    listening_channel_count: i64,
    temporary_relation_is_missing: bool,
    prepared_statement: Result<tokio_postgres::Row, tokio_postgres::Error>,
    cursor_fetch: Result<Vec<tokio_postgres::Row>, tokio_postgres::Error>,
}

async fn observe_discard_all_state(
    client: &tokio_postgres::Client,
    contender: &tokio_postgres::Client,
    lock_before_reset: bool,
) -> DiscardAllObservations {
    let application_name = client
        .query_one("SHOW application_name", &[])
        .await
        .expect("read application_name after DISCARD ALL")
        .get(0);
    let cursor_count = client
        .query_one(
            "SELECT count(*) FROM pg_cursors WHERE name = 'compat_discard_cursor'",
            &[],
        )
        .await
        .expect("inspect cursor cleanup after DISCARD ALL")
        .get(0);
    let listening_channel_count = client
        .query_one(
            "SELECT count(*) FROM pg_listening_channels() \
             WHERE pg_listening_channels = 'compat_discard_channel'",
            &[],
        )
        .await
        .expect("inspect listener cleanup after DISCARD ALL")
        .get(0);
    let temporary_relation_is_missing = client
        .query_one(
            "SELECT count(*) = 0 FROM pg_catalog.pg_class \
             WHERE relname = 'compat_discard_temp'",
            &[],
        )
        .await
        .expect("check that DISCARD ALL removed the session temp relation")
        .get(0);
    let prepared_statement = client
        .query_one("EXECUTE compat_discard_prepared(41)", &[])
        .await;
    let cursor_fetch = client
        .query("FETCH FORWARD 1 FROM compat_discard_cursor", &[])
        .await;
    let lock_after_reset = contender
        .query_one("SELECT pg_try_advisory_lock(82182101)", &[])
        .await
        .expect("probe the lock after DISCARD ALL")
        .get(0);
    let contender_unlocked = contender
        .query_one("SELECT pg_advisory_unlock(82182101)", &[])
        .await
        .expect("release the lock acquired after reset")
        .get(0);
    DiscardAllObservations {
        advisory_locks: AdvisoryLockObservations {
            could_acquire_before_reset: lock_before_reset,
            could_acquire_after_reset: lock_after_reset,
            released_after_reset: contender_unlocked,
        },
        application_name,
        cursor_count,
        listening_channel_count,
        temporary_relation_is_missing,
        prepared_statement,
        cursor_fetch,
    }
}

#[test]
fn should_reset_connection_state_with_discard_all() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("discard_all_connection_state").await;
        let (client, connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        client
            .batch_execute(
                "SET application_name = 'discard-all-probe'; \
                 LISTEN compat_discard_channel; \
                 CREATE TEMP TABLE compat_discard_temp (id INT PRIMARY KEY); \
                 INSERT INTO compat_discard_temp VALUES (1), (2); \
                 PREPARE compat_discard_prepared (INT) AS SELECT $1 + 1",
            )
            .await
            .expect("create session settings, prepared state, a listener, temp data and a cursor");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin the holdable cursor transaction");
        client
            .batch_execute(
                "DECLARE compat_discard_cursor CURSOR WITH HOLD FOR \
                 SELECT id FROM compat_discard_temp ORDER BY id",
            )
            .await
            .expect("declare a holdable cursor over the temporary rows");
        client
            .batch_execute("COMMIT")
            .await
            .expect("keep the holdable cursor after its transaction commits");
        client
            .query_one("SELECT pg_advisory_lock(82182101)", &[])
            .await
            .expect("acquire a session-level advisory lock before reset");
        let lock_before_reset: bool = contender
            .query_one("SELECT pg_try_advisory_lock(82182101)", &[])
            .await
            .expect("probe the lock from a second session")
            .get(0);

        // Act
        client
            .batch_execute("DISCARD ALL")
            .await
            .expect("discard every resettable piece of session state");
        let observations = observe_discard_all_state(&client, &contender, lock_before_reset).await;

        // Assert
        assert_discard_all_state(&observations);

        drop((client, contender));
        server.shutdown(connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
    });
}

#[test]
fn should_reset_session_parameter_with_discard_all() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("discard_all_session_parameter").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("SET application_name = 'compat-discard-all-probe'")
            .await
            .expect("set a session parameter before resetting connection state");

        // Act
        client
            .batch_execute("DISCARD ALL")
            .await
            .expect("PostgreSQL 18.6 accepts DISCARD ALL outside a transaction");
        let application_name: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read the session parameter after DISCARD ALL")
            .get(0);

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(application_name, "");
    });
}
