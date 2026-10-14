use super::*;

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
        let application_name: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read application_name after DISCARD ALL")
            .get(0);
        let cursor_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_cursors WHERE name = 'compat_discard_cursor'",
                &[],
            )
            .await
            .expect("inspect cursor cleanup after DISCARD ALL")
            .get(0);
        let listening_channel_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_listening_channels() \
                 WHERE pg_listening_channels = 'compat_discard_channel'",
                &[],
            )
            .await
            .expect("inspect listener cleanup after DISCARD ALL")
            .get(0);
        let temporary_relation_is_missing: bool = client
            .query_one("SELECT to_regclass('compat_discard_temp') IS NULL", &[])
            .await
            .expect("check that DISCARD ALL removed the session temp relation")
            .get(0);
        let prepared_statement = client
            .query_one("EXECUTE compat_discard_prepared(41)", &[])
            .await;
        let cursor_fetch = client
            .query("FETCH FORWARD 1 FROM compat_discard_cursor", &[])
            .await;
        let lock_after_reset: bool = contender
            .query_one("SELECT pg_try_advisory_lock(82182101)", &[])
            .await
            .expect("probe the lock after DISCARD ALL")
            .get(0);
        let contender_unlocked: bool = contender
            .query_one("SELECT pg_advisory_unlock(82182101)", &[])
            .await
            .expect("release the lock acquired after reset")
            .get(0);

        // Assert
        assert!(!lock_before_reset);
        assert_eq!(application_name, "");
        assert_eq!(cursor_count, 0);
        assert_eq!(listening_channel_count, 0);
        assert!(temporary_relation_is_missing);
        assert_eq!(
            db_error(&prepared_statement.expect_err("DISCARD ALL deallocates SQL PREPARE state"))
                .code()
                .code(),
            "26000"
        );
        assert_eq!(
            db_error(&cursor_fetch.expect_err("DISCARD ALL closes open cursors"))
                .code()
                .code(),
            "34000"
        );
        assert!(lock_after_reset);
        assert!(contender_unlocked);

        drop((client, contender));
        server.shutdown(connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
    });
}
