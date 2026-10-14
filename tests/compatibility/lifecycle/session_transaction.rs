use super::*;

mod row_lock_matrix;
mod timezone_settings;

#[test]
fn should_execute_sql_level_prepared_statements_then_deallocate_them() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sql_prepared_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_sql_prepared (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_sql_prepared VALUES (1, 'alpha'), (2, 'beta')",
                "PREPARE compat_find_label(INT) AS \
                   SELECT label FROM compat_sql_prepared WHERE id = $1",
            ],
        )
        .await
        .expect("SQL PREPARE should register a statement");

        // Act
        let first = client
            .query_one("EXECUTE compat_find_label(1)", &[])
            .await
            .expect("execute prepared SQL for the first input");
        let second = client
            .query_one("EXECUTE compat_find_label(2)", &[])
            .await
            .expect("execute prepared SQL for the second input");
        client
            .batch_execute("DEALLOCATE compat_find_label")
            .await
            .expect("deallocate SQL statement");
        let missing = client
            .query_one("EXECUTE compat_find_label(1)", &[])
            .await
            .expect_err("deallocated statement must not execute");

        // Assert
        let first_label: String = first.try_get(0).expect("first label");
        let second_label: String = second.try_get(0).expect("second label");
        assert_eq!(first_label, "alpha");
        assert_eq!(second_label, "beta");
        assert_eq!(db_error(&missing).code().code(), "26000");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_clear_extended_prepared_statement_with_deallocate_all() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("deallocate_all_prepared").await;
        let (client, connection) = server.connect().await;
        let prepared = client
            .prepare("SELECT $1::INT")
            .await
            .expect("prepare a statement through the extended query protocol");
        let before: i32 = client
            .query_one(&prepared, &[&41_i32])
            .await
            .expect("execute the named prepared statement before deallocation")
            .try_get(0)
            .expect("prepared statement value");

        // Act
        client
            .batch_execute("DEALLOCATE ALL")
            .await
            .expect("deallocate every SQL and extended-protocol prepared statement");
        let missing = client
            .query_one(&prepared, &[&41_i32])
            .await
            .expect_err("DEALLOCATE ALL removes the extended-protocol statement");

        // Assert
        assert_eq!(before, 41);
        assert_eq!(db_error(&missing).code().code(), "26000");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_keep_repeatable_read_snapshot_stable_across_other_session_commits() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("repeatable_read_snapshot").await;
        let (reader, reader_connection) = server.connect().await;
        let (writer, writer_connection) = server.connect().await;
        execute_statements(
            &reader,
            &[
                "CREATE TABLE compat_snapshot_rows (id INT PRIMARY KEY)",
                "INSERT INTO compat_snapshot_rows VALUES (1)",
            ],
        )
        .await
        .expect("create the snapshot fixture");
        let begin = reader
            .batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ")
            .await;
        // Act
        let counts = if begin.is_ok() {
            let before = reader
                .query_one("SELECT count(*) FROM compat_snapshot_rows", &[])
                .await
                .expect("establish the transaction snapshot")
                .get::<_, i64>(0);
            writer
                .batch_execute("INSERT INTO compat_snapshot_rows VALUES (2)")
                .await
                .expect("commit a concurrent row");
            let during = reader
                .query_one("SELECT count(*) FROM compat_snapshot_rows", &[])
                .await
                .expect("read through the existing snapshot")
                .get::<_, i64>(0);
            reader
                .batch_execute("COMMIT")
                .await
                .expect("finish the repeatable read transaction");
            let after = reader
                .query_one("SELECT count(*) FROM compat_snapshot_rows", &[])
                .await
                .expect("read with a fresh snapshot")
                .get::<_, i64>(0);
            Some((before, during, after))
        } else {
            None
        };
        drop((reader, writer));
        server.shutdown(reader_connection).await;
        writer_connection.abort();
        let _ = writer_connection.await;

        // Assert
        assert!(
            begin.is_ok(),
            "REPEATABLE READ must start successfully: {begin:?}"
        );
        assert_eq!(counts.expect("snapshot observations"), (1, 1, 2));
    });
}

#[test]
fn should_wait_for_unique_key_owner_before_returning_pgwire_insert() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        use cassie::config::CassieRuntimeConfig;
        use std::sync::Arc;

        support::use_local_storage();
        let path = support::data_dir("unique_key_transaction_wait");
        let _directory = DataDirectory(path.clone());
        let mut server_config = CassieRuntimeConfig::from_env().expect("runtime config");
        server_config.password = "postgres".to_string();
        let cassie = Cassie::new_with_data_dir_and_config(&path, server_config.clone())
            .expect("create instrumented Cassie instance");
        cassie
            .startup()
            .expect("start instrumented Cassie instance");
        let server_app = Arc::new(cassie.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind pgwire listener");
        let addr = listener.local_addr().expect("pgwire listener address");
        drop(listener);
        let server = tokio::spawn(async move {
            let _ = cassie::pgwire::server::run(addr.to_string(), server_app, server_config).await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut connection_config = tokio_postgres::Config::new();
        connection_config
            .host("127.0.0.1")
            .port(addr.port())
            .user("root")
            .password("postgres")
            .dbname("postgres");
        let (owner, owner_connection) = connection_config
            .connect(NoTls)
            .await
            .expect("connect owner session");
        let owner_connection = tokio::spawn(async move {
            owner_connection
                .await
                .expect("owner pgwire connection should stay healthy");
        });
        let (contender, contender_connection) = connection_config
            .connect(NoTls)
            .await
            .expect("connect competing session");
        let contender_connection = tokio::spawn(async move {
            contender_connection
                .await
                .expect("competing pgwire connection should stay healthy");
        });
        owner
            .batch_execute(
                "CREATE TABLE compat_unique_wait_rows (id INT PRIMARY KEY, value TEXT UNIQUE)",
            )
            .await
            .expect("create a unique-key transaction fixture");
        owner
            .batch_execute("BEGIN; INSERT INTO compat_unique_wait_rows VALUES (1, 'shared')")
            .await
            .expect("hold the unique value in an open transaction");
        let mut contender_insert = tokio::spawn(async move {
            contender
                .execute(
                    "INSERT INTO compat_unique_wait_rows VALUES (2, 'shared')",
                    &[],
                )
                .await
        });

        // Act
        // Observe the executing query in runtime metrics before testing whether it completes.
        let request_entered_engine = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if contender_insert.is_finished() {
                    return false;
                }
                if cassie.metrics()["runtime"]["running_queries"]
                    .as_u64()
                    .is_some_and(|running| running > 0)
                {
                    return true;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or(false);
        let early_completion =
            tokio::time::timeout(Duration::from_millis(500), &mut contender_insert)
                .await
                .ok();
        let completed_before_commit = early_completion.is_some();
        assert!(
            request_entered_engine || completed_before_commit,
            "the competing query must reach the engine or complete before owner resolution"
        );
        let owner_commit = owner.batch_execute("COMMIT").await;
        let contender_result = if let Some(result) = early_completion {
            result.expect("competing INSERT task should complete")
        } else {
            contender_insert
                .await
                .expect("competing INSERT task should complete after owner resolution")
        };

        // Assert
        drop(owner);
        contender_connection.abort();
        let _ = contender_connection.await;
        owner_connection.abort();
        let _ = owner_connection.await;
        server.abort();
        let _ = server.await;
        assert!(
            !completed_before_commit,
            "PostgreSQL waits for the in-flight pgwire INSERT before owner resolution; \
             engine entry observed: {request_entered_engine}; \
             completed before owner COMMIT: {completed_before_commit}; \
             owner COMMIT: {owner_commit:?}; competing INSERT: {contender_result:?}"
        );
        owner_commit.expect("the transaction that inserted the unique value commits");
        let error = contender_result.expect_err("the committed owner keeps the unique value");
        assert_eq!(db_error(&error).code().code(), "23505");
    });
}

#[test]
fn should_restore_set_local_state_after_commit() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("set_local_setting_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "SET application_name = 'session-value'",
                "BEGIN",
                "SET LOCAL application_name = 'transaction-value'",
            ],
        )
        .await
        .expect("set transaction-local application_name");
        let during: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read local setting")
            .try_get(0)
            .expect("setting text");

        // Act
        client
            .batch_execute("COMMIT")
            .await
            .expect("commit setting transaction");
        let after: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read restored setting")
            .try_get(0)
            .expect("setting text");
        // Assert
        assert_eq!(during, "transaction-value");
        assert_eq!(after, "session-value");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_discard_session_settings_clear_wire_prepared_state() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("discard_session_state").await;
        let (client, connection) = server.connect().await;
        execute_statements(&client, &["SET application_name = 'discard-me'"])
            .await
            .expect("set session state before reset");
        let prepared = client
            .prepare("SELECT $1::INT")
            .await
            .expect("prepare wire statement before reset");

        // Act
        client
            .batch_execute("DISCARD ALL")
            .await
            .expect("discard all session state");
        let application_name: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read reset application name")
            .try_get(0)
            .expect("application name");
        let missing = client
            .query_one(&prepared, &[&1_i32])
            .await
            .expect_err("DISCARD ALL must clear wire prepared statements");

        // Assert
        assert_eq!(application_name, "");
        assert_eq!(db_error(&missing).code().code(), "26000");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reset_all_session_settings_to_their_startup_values() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reset_all_settings").await;
        let (client, connection) = server.connect().await;
        let startup_application_name: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read startup application_name")
            .try_get(0)
            .expect("application_name text");
        client
            .batch_execute("SET application_name = 'temporary-session-value'")
            .await
            .expect("change a session setting before reset");

        // Act
        client
            .batch_execute("RESET ALL")
            .await
            .expect("reset all configurable session settings");
        let reset_application_name: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read application_name after reset")
            .try_get(0)
            .expect("application_name text");

        // Assert
        assert_eq!(reset_application_name, startup_application_name);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_complete_sql_cursor_lifecycle_fetch_move_close() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("cursor_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_cursor_rows (id INT PRIMARY KEY)",
                "INSERT INTO compat_cursor_rows VALUES (1), (2), (3)",
                "BEGIN",
                "DECLARE compat_cursor NO SCROLL CURSOR FOR \
                   SELECT id FROM compat_cursor_rows ORDER BY id",
            ],
        )
        .await
        .expect("declare cursor over ordered fixture rows");

        // Act
        let first_page = client
            .query("FETCH FORWARD 1 FROM compat_cursor", &[])
            .await
            .expect("fetch first cursor row");
        client
            .batch_execute("MOVE FORWARD 1 FROM compat_cursor")
            .await
            .expect("move cursor over one row");
        let last_page = client
            .query("FETCH FORWARD 1 FROM compat_cursor", &[])
            .await
            .expect("fetch row after move");
        client
            .batch_execute("CLOSE compat_cursor")
            .await
            .expect("close cursor");
        client
            .batch_execute("COMMIT")
            .await
            .expect("commit cursor transaction");
        let closed = client
            .query("FETCH FORWARD 1 FROM compat_cursor", &[])
            .await
            .expect_err("closed cursor must not fetch");

        // Assert
        let first_ids = first_page
            .iter()
            .map(|row| row.get::<_, i32>(0))
            .collect::<Vec<_>>();
        let last_ids = last_page
            .iter()
            .map(|row| row.get::<_, i32>(0))
            .collect::<Vec<_>>();
        assert_eq!(first_ids, vec![1]);
        assert_eq!(last_ids, vec![3]);
        assert_eq!(db_error(&closed).code().code(), "34000");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_keep_a_holdable_cursor_open_after_commit() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("holdable_cursor_commit").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_holdable_rows (id INT PRIMARY KEY)")
            .await
            .expect("create holdable cursor fixture table");
        client
            .batch_execute("INSERT INTO compat_holdable_rows VALUES (1), (2), (3)")
            .await
            .expect("seed holdable cursor fixture rows");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin holdable cursor transaction");
        client
            .batch_execute(
                "DECLARE compat_holdable CURSOR WITH HOLD FOR \
                 SELECT id FROM compat_holdable_rows ORDER BY id",
            )
            .await
            .expect("declare holdable cursor inside a transaction");
        let first_page = client
            .query("FETCH FORWARD 1 FROM compat_holdable", &[])
            .await
            .expect("fetch before commit");

        // Act
        client
            .batch_execute("COMMIT")
            .await
            .expect("commit while preserving a holdable cursor");
        let remaining_page = client
            .query("FETCH FORWARD 2 FROM compat_holdable", &[])
            .await
            .expect("fetch remaining cursor rows after commit");
        client
            .batch_execute("CLOSE compat_holdable")
            .await
            .expect("close holdable cursor after reading it");

        // Assert
        assert_eq!(
            first_page
                .iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(
            remaining_page
                .iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![2, 3]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

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
fn should_enforce_nowait_skip_locked_row_claims_across_sessions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("row_lock_claims").await;
        let (owner, owner_connection) = server.connect().await;
        let (worker, worker_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TABLE compat_claim_rows (id INT PRIMARY KEY)",
                "INSERT INTO compat_claim_rows VALUES (1)",
                "BEGIN",
            ],
        )
        .await
        .expect("seed and begin row-lock owner");
        let held: i32 = owner
            .query_one("SELECT id FROM compat_claim_rows FOR UPDATE", &[])
            .await
            .expect("lock claim row")
            .try_get(0)
            .expect("locked id");

        // Act
        let nowait = worker
            .query("SELECT id FROM compat_claim_rows FOR UPDATE NOWAIT", &[])
            .await
            .expect_err("NOWAIT must fail while another session owns the row");
        let skipped = worker
            .query(
                "SELECT id FROM compat_claim_rows FOR UPDATE SKIP LOCKED",
                &[],
            )
            .await
            .expect("SKIP LOCKED should omit the busy row");
        owner
            .batch_execute("COMMIT")
            .await
            .expect("release row lock");
        let claimed: i32 = worker
            .query_one("SELECT id FROM compat_claim_rows FOR UPDATE NOWAIT", &[])
            .await
            .expect("claim row after release")
            .try_get(0)
            .expect("claimed id");

        // Assert
        assert_eq!(held, 1);
        assert_eq!(db_error(&nowait).code().code(), "55P03");
        assert!(skipped.is_empty());
        assert_eq!(claimed, 1);

        drop((owner, worker));
        server.shutdown(owner_connection).await;
        worker_connection.abort();
        let _ = worker_connection.await;
    });
}

#[test]
fn should_conflict_for_update_nowait_with_a_for_share_row_lock() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("for_share_row_lock").await;
        let (owner, owner_connection) = server.connect().await;
        let (worker, worker_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TABLE compat_share_lock_rows (id INT PRIMARY KEY)",
                "INSERT INTO compat_share_lock_rows VALUES (1)",
                "BEGIN",
            ],
        )
        .await
        .expect("create the row-lock fixture");
        let shared = owner
            .query_one("SELECT id FROM compat_share_lock_rows FOR SHARE", &[])
            .await;

        // Act
        let conflicting = if shared.is_ok() {
            Some(
                worker
                    .query(
                        "SELECT id FROM compat_share_lock_rows FOR UPDATE NOWAIT",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };
        owner
            .batch_execute("ROLLBACK")
            .await
            .expect("release the owner's row lock");
        let retry = if shared.is_ok() {
            Some(
                worker
                    .query_one(
                        "SELECT id FROM compat_share_lock_rows FOR UPDATE NOWAIT",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };
        drop((owner, worker));
        server.shutdown(owner_connection).await;
        worker_connection.abort();
        let _ = worker_connection.await;

        // Assert
        assert!(
            shared.is_ok(),
            "FOR SHARE must acquire the row lock: {shared:?}"
        );
        let conflict = conflicting
            .expect("worker lock result")
            .expect_err("FOR UPDATE NOWAIT must conflict with another session's FOR SHARE lock");
        assert_eq!(db_error(&conflict).code().code(), "55P03");
        assert_eq!(
            retry
                .expect("post-rollback claim result")
                .expect("claim row after the shared lock is released")
                .get::<_, i32>(0),
            1
        );
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
fn should_keep_two_key_transaction_advisory_locks_until_commit() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("two_key_xact_advisory_lock").await;
        let (owner, owner_connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin lock-owner transaction");
        owner
            .batch_execute("SELECT pg_advisory_xact_lock(71, 29)")
            .await
            .expect("acquire a two-key transaction advisory lock");

        // Act
        let blocked: bool = contender
            .query_one("SELECT pg_try_advisory_xact_lock(71, 29)", &[])
            .await
            .expect("try the two-key lock while the owner holds it")
            .try_get(0)
            .expect("contended two-key lock result");
        owner
            .batch_execute("COMMIT")
            .await
            .expect("release transaction advisory lock at commit");
        let available: bool = contender
            .query_one("SELECT pg_try_advisory_xact_lock(71, 29)", &[])
            .await
            .expect("acquire the two-key lock after owner commit")
            .try_get(0)
            .expect("available two-key lock result");
        contender
            .batch_execute("COMMIT")
            .await
            .expect("release the contender's transaction advisory lock");

        // Assert
        assert!(!blocked);
        assert!(available);

        drop(owner);
        drop(contender);
        server.shutdown(owner_connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
    });
}

#[test]
fn should_discard_sequence_session_state_without_rewinding_the_sequence() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("discard_sequence_state").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SEQUENCE compat_discard_sequence")
            .await
            .expect("create a sequence for session-state reset");
        let first: i64 = client
            .query_one("SELECT nextval('compat_discard_sequence')", &[])
            .await
            .expect("read the first sequence value")
            .try_get(0)
            .expect("first sequence value");

        // Act
        client
            .batch_execute("DISCARD SEQUENCES")
            .await
            .expect("discard this session's sequence state");
        let forgotten = client
            .query_one("SELECT currval('compat_discard_sequence')", &[])
            .await
            .expect_err("currval must be unset after DISCARD SEQUENCES");
        let next: i64 = client
            .query_one("SELECT nextval('compat_discard_sequence')", &[])
            .await
            .expect("continue allocating from the sequence")
            .try_get(0)
            .expect("next sequence value");

        // Assert
        assert_eq!(first, 1);
        assert_eq!(db_error(&forgotten).code().code(), "55000");
        assert_eq!(next, 2);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_deliver_notifications_after_commit_once_per_duplicate_payload() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("notify_commit_dedup").await;
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
            .expect("connect the notification listener");
        let (notification_sender, mut notifications) = tokio::sync::mpsc::unbounded_channel();
        let notification_task = tokio::spawn(async move {
            loop {
                match std::future::poll_fn(|context| listener_connection.poll_message(context))
                    .await
                {
                    Some(Ok(tokio_postgres::AsyncMessage::Notification(notification))) => {
                        if notification_sender
                            .send((
                                notification.channel().to_string(),
                                notification.payload().to_string(),
                            ))
                            .is_err()
                        {
                            return;
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) | None => return,
                }
            }
        });
        listener
            .batch_execute("LISTEN compat_commit_events")
            .await
            .expect("subscribe before sending notifications");
        let (sender, sender_connection) = server.connect().await;

        // Act
        sender
            .batch_execute("BEGIN; NOTIFY compat_commit_events, 'rolled-back'; ROLLBACK")
            .await
            .expect("roll back a notification transaction");
        let rolled_back =
            tokio::time::timeout(Duration::from_millis(250), notifications.recv()).await;
        sender
            .batch_execute(
                "BEGIN; NOTIFY compat_commit_events, 'same'; \
                 NOTIFY compat_commit_events, 'same'; COMMIT",
            )
            .await
            .expect("commit duplicate notifications");
        let delivered = tokio::time::timeout(Duration::from_secs(2), notifications.recv())
            .await
            .expect("committed notification should arrive")
            .expect("notification channel should remain open");
        let duplicate =
            tokio::time::timeout(Duration::from_millis(250), notifications.recv()).await;

        // Assert
        assert!(
            rolled_back.is_err(),
            "rolled-back notifications must not be delivered"
        );
        assert_eq!(
            delivered,
            ("compat_commit_events".to_string(), "same".to_string())
        );
        assert!(
            duplicate.is_err(),
            "identical notifications in one transaction are folded"
        );

        notification_task.abort();
        drop((listener, sender));
        server.shutdown(sender_connection).await;
        let _ = notification_task.await;
    });
}

#[test]
fn should_remove_session_temporary_tables_when_discarding_temp() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("discard_temp_objects").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_discard_temp_persistent (id INT PRIMARY KEY)",
                "INSERT INTO compat_discard_temp_persistent VALUES (1)",
                "CREATE TEMP TABLE compat_discard_temp_temporary (id INT)",
                "INSERT INTO compat_discard_temp_temporary VALUES (2)",
            ],
        )
        .await
        .expect("create permanent and session temporary tables");

        // Act
        client
            .batch_execute("DISCARD TEMP")
            .await
            .expect("discard current-session temporary objects");
        let persistent_count: i64 = client
            .query_one("SELECT count(*) FROM compat_discard_temp_persistent", &[])
            .await
            .expect("permanent table should remain usable")
            .try_get(0)
            .expect("persistent row count");
        let temporary_missing = client
            .query("SELECT id FROM compat_discard_temp_temporary", &[])
            .await
            .expect_err("DISCARD TEMP should remove the current-session table");

        // Assert
        assert_eq!(persistent_count, 1);
        assert_eq!(db_error(&temporary_missing).code().code(), "42P01");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_isolate_temporary_views_to_the_owning_session() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("temporary_view_isolation").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        owner
            .batch_execute("CREATE TABLE compat_temp_view_source (id INT)")
            .await
            .expect("create a permanent source table for the temporary view");
        owner
            .batch_execute("INSERT INTO compat_temp_view_source VALUES (1)")
            .await
            .expect("seed the temporary-view source table");
        owner
            .batch_execute(
                "CREATE TEMP VIEW compat_temp_session_view AS \
                 SELECT id FROM compat_temp_view_source",
            )
            .await
            .expect("create a session-local view over the permanent source table");

        // Act
        let visible = owner
            .query_one("SELECT id FROM compat_temp_session_view", &[])
            .await
            .expect("read the view in the owning session");
        let invisible = observer
            .query_one("SELECT id FROM compat_temp_session_view", &[])
            .await
            .expect_err("another session must not resolve a temporary view");

        // Assert
        assert_eq!(visible.get::<_, i32>(0), 1);
        assert_eq!(db_error(&invisible).code().code(), "42P01");

        drop((owner, observer));
        server.shutdown(owner_connection).await;
        observer_connection.abort();
        let _ = observer_connection.await;
    });
}

#[test]
fn should_enforce_read_only_transaction_mode_with_postgres_sqlstate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("read_only_transaction_mode").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_read_only_rows (id INT PRIMARY KEY)",
                "INSERT INTO compat_read_only_rows VALUES (1)",
            ],
        )
        .await
        .expect("create the read-only transaction fixture");
        client
            .batch_execute("BEGIN TRANSACTION READ ONLY")
            .await
            .expect("begin a PostgreSQL read-only transaction");

        // Act
        let visible_rows = client
            .query("SELECT id FROM compat_read_only_rows ORDER BY id", &[])
            .await
            .expect("read-only transactions should allow SELECT");
        let rejected_write = client
            .execute("INSERT INTO compat_read_only_rows VALUES (2)", &[])
            .await
            .expect_err("read-only transactions must reject writes");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back after the rejected write");
        let stored_rows = client
            .query("SELECT id FROM compat_read_only_rows ORDER BY id", &[])
            .await
            .expect("read the committed fixture after rollback");

        // Assert
        assert_eq!(visible_rows.len(), 1);
        assert_eq!(visible_rows[0].get::<_, i32>(0), 1);
        assert_eq!(db_error(&rejected_write).code().code(), "25006");
        assert_eq!(stored_rows.len(), 1);
        assert_eq!(stored_rows[0].get::<_, i32>(0), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_begin_followup_transaction_when_commit_chaining() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("commit_and_chain").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_chain_rows (id INT PRIMARY KEY)")
            .await
            .expect("create the transaction-chain fixture");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin the first transaction");
        client
            .batch_execute("INSERT INTO compat_chain_rows VALUES (1)")
            .await
            .expect("write in the first transaction");

        // Act
        client
            .batch_execute("COMMIT AND CHAIN")
            .await
            .expect("commit and begin the next transaction");
        client
            .batch_execute("INSERT INTO compat_chain_rows VALUES (2)")
            .await
            .expect("write in the chained transaction");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back only the chained transaction");
        let rows = client
            .query("SELECT id FROM compat_chain_rows ORDER BY id", &[])
            .await
            .expect("read committed rows after rolling back the chain");

        // Assert
        assert_eq!(
            rows.iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![1]
        );

        drop(client);
        server.shutdown(connection).await;
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
fn should_keep_extended_prepared_statement_after_discard_plans() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("discard_prepared_plans").await;
        let (client, connection) = server.connect().await;
        let statement = client
            .prepare("SELECT 7")
            .await
            .expect("prepare the extended-protocol plan-cache fixture");
        let before_discard: i32 = client
            .query_one(&statement, &[])
            .await
            .expect("execute the prepared statement before discard")
            .try_get(0)
            .expect("first result");

        // Act
        client
            .batch_execute("DISCARD PLANS")
            .await
            .expect("discard cached plans without deallocating statements");
        let after_discard: i32 = client
            .query_one(&statement, &[])
            .await
            .expect("execute the retained prepared statement after discard")
            .try_get(0)
            .expect("second result");

        // Assert
        assert_eq!(before_discard, 7);
        assert_eq!(after_discard, 7);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_abort_one_serializable_transaction_after_write_skew() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("serializable_write_skew").await;
        let (left, left_connection) = server.connect().await;
        let (right, right_connection) = server.connect().await;
        execute_statements(
            &left,
            &[
                "CREATE TABLE compat_serializable_on_call \
                   (doctor_id INT PRIMARY KEY, on_call BOOLEAN NOT NULL)",
                "INSERT INTO compat_serializable_on_call VALUES (1, true), (2, true)",
            ],
        )
        .await
        .expect("create the write-skew fixture");
        let left_begin = left
            .batch_execute("BEGIN ISOLATION LEVEL SERIALIZABLE")
            .await;
        let right_begin = right
            .batch_execute("BEGIN ISOLATION LEVEL SERIALIZABLE")
            .await;

        // Act
        let outcome = if left_begin.is_ok() && right_begin.is_ok() {
            let left_snapshot = left
                .query_one(
                    "SELECT count(*) FROM compat_serializable_on_call WHERE on_call",
                    &[],
                )
                .await
                .expect("read the left transaction snapshot")
                .get::<_, i64>(0);
            let right_snapshot = right
                .query_one(
                    "SELECT count(*) FROM compat_serializable_on_call WHERE on_call",
                    &[],
                )
                .await
                .expect("read the right transaction snapshot")
                .get::<_, i64>(0);
            let left_update = left
                .execute(
                    "UPDATE compat_serializable_on_call SET on_call = false \
                     WHERE doctor_id = 1",
                    &[],
                )
                .await;
            let right_update = right
                .execute(
                    "UPDATE compat_serializable_on_call SET on_call = false \
                     WHERE doctor_id = 2",
                    &[],
                )
                .await;
            let left_commit = left.batch_execute("COMMIT").await;
            let right_commit = right.batch_execute("COMMIT").await;
            let remaining_on_call: i64 = left
                .query_one(
                    "SELECT count(*) FROM compat_serializable_on_call WHERE on_call",
                    &[],
                )
                .await
                .expect("inspect the committed write-skew outcome")
                .get(0);
            Some((
                left_snapshot,
                right_snapshot,
                left_update,
                right_update,
                left_commit,
                right_commit,
                remaining_on_call,
            ))
        } else {
            None
        };

        // Assert
        let left_started = left_begin.is_ok();
        let right_started = right_begin.is_ok();
        drop((left, right));
        server.shutdown(left_connection).await;
        right_connection.abort();
        let _ = right_connection.await;
        assert!(
            left_started,
            "left SERIALIZABLE transaction: {left_begin:?}"
        );
        assert!(
            right_started,
            "right SERIALIZABLE transaction: {right_begin:?}"
        );
        let (
            left_snapshot,
            right_snapshot,
            left_update,
            right_update,
            left_commit,
            right_commit,
            remaining_on_call,
        ) = outcome.expect("both serializable transactions should observe a snapshot");
        let left_aborted_for_serialization = left_update
            .as_ref()
            .is_err_and(|error| db_error(error).code().code() == "40001")
            || left_commit
                .as_ref()
                .is_err_and(|error| db_error(error).code().code() == "40001");
        let right_aborted_for_serialization = right_update
            .as_ref()
            .is_err_and(|error| db_error(error).code().code() == "40001")
            || right_commit
                .as_ref()
                .is_err_and(|error| db_error(error).code().code() == "40001");
        assert_eq!(left_snapshot, 2);
        assert_eq!(right_snapshot, 2);
        assert!(
            left_aborted_for_serialization || right_aborted_for_serialization,
            "one transaction must fail with SQLSTATE 40001; left update/commit: \
             {left_update:?}/{left_commit:?}; right update/commit: \
             {right_update:?}/{right_commit:?}"
        );
        assert_eq!(remaining_on_call, 1);
    });
}

#[test]
fn should_conflict_table_locks_across_sessions_with_nowait() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("table_lock_nowait").await;
        let (owner, owner_connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        owner
            .batch_execute("CREATE TABLE compat_table_lock_rows (id INT)")
            .await
            .expect("create a table lock fixture");

        // Act
        let owner_lock = execute_statements(
            &owner,
            &[
                "BEGIN",
                "LOCK TABLE compat_table_lock_rows IN ACCESS SHARE MODE",
            ],
        )
        .await;
        let conflict = if owner_lock.is_ok() {
            contender
                .batch_execute("BEGIN")
                .await
                .expect("begin contender");
            Some(
                contender
                    .batch_execute(
                        "LOCK TABLE compat_table_lock_rows \
                         IN ACCESS EXCLUSIVE MODE NOWAIT",
                    )
                    .await,
            )
        } else {
            None
        };
        if owner_lock.is_ok() {
            owner
                .batch_execute("ROLLBACK")
                .await
                .expect("release the owner's table lock");
        }
        let retry = if conflict.is_some() {
            contender
                .batch_execute("ROLLBACK")
                .await
                .expect("clear the failed lock transaction");
            execute_statements(
                &contender,
                &[
                    "BEGIN",
                    "LOCK TABLE compat_table_lock_rows \
                     IN ACCESS EXCLUSIVE MODE NOWAIT",
                    "COMMIT",
                ],
            )
            .await
        } else {
            Ok(())
        };

        // Assert
        drop((owner, contender));
        server.shutdown(owner_connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
        assert!(owner_lock.is_ok(), "owner table lock: {owner_lock:?}");
        let conflict = conflict.expect("contender should attempt its conflicting lock");
        assert_eq!(
            db_error(
                conflict
                    .as_ref()
                    .expect_err("NOWAIT must fail while ACCESS SHARE is held"),
            )
            .code()
            .code(),
            "55P03"
        );
        assert!(
            retry.is_ok(),
            "the lock should succeed after release: {retry:?}"
        );
    });
}

#[test]
fn should_enforce_default_transaction_read_only_for_table_writes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("default_transaction_read_only").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_read_only_rows (id INT PRIMARY KEY)",
                "INSERT INTO compat_read_only_rows VALUES (1)",
            ],
        )
        .await
        .expect("create the read-only transaction fixture");

        // Act
        let setting = client
            .batch_execute("SET default_transaction_read_only = on")
            .await;
        let outcome = if setting.is_ok() {
            let begin = client.batch_execute("BEGIN").await;
            if begin.is_ok() {
                let write = client
                    .execute("INSERT INTO compat_read_only_rows VALUES (2)", &[])
                    .await;
                client
                    .batch_execute("ROLLBACK")
                    .await
                    .expect("finish the read-only transaction");
                let rows: i64 = client
                    .query_one("SELECT count(*) FROM compat_read_only_rows", &[])
                    .await
                    .expect("inspect rows after the rejected write")
                    .get(0);
                Some((write, rows))
            } else {
                None
            }
        } else {
            None
        };

        // Assert
        let setting_succeeded = setting.is_ok();
        drop(client);
        server.shutdown(connection).await;
        assert!(
            setting_succeeded,
            "default_transaction_read_only should be accepted: {setting:?}"
        );
        let (write, rows) = outcome.expect("read-only transaction should start");
        assert_eq!(
            db_error(
                write
                    .as_ref()
                    .expect_err("read-only transactions reject table writes"),
            )
            .code()
            .code(),
            "25006"
        );
        assert_eq!(rows, 1);
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

#[test]
fn should_return_void_from_pg_notify_without_a_listener() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pg_notify_function").await;
        let (client, connection) = server.connect().await;

        // Act
        let notification = client
            .query_one(
                "SELECT pg_notify('compat_pg_notify_function', 'payload-18')::TEXT",
                &[],
            )
            .await;

        // Assert
        let notification = notification.expect("pg_notify returns PostgreSQL VOID");
        assert_eq!(notification.get::<_, String>(0), "");

        drop(client);
        server.shutdown(connection).await;
    });
}
