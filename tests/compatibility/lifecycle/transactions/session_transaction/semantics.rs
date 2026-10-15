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
fn should_refresh_read_committed_snapshot_after_other_session_commit() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("read_committed_new_snapshot").await;
        let (reader, reader_connection) = server.connect().await;
        let (writer, writer_connection) = server.connect().await;
        execute_statements(
            &reader,
            &[
                "CREATE TABLE compat_read_committed_rows (id INT PRIMARY KEY)",
                "INSERT INTO compat_read_committed_rows VALUES (1)",
                "BEGIN ISOLATION LEVEL READ COMMITTED",
            ],
        )
        .await
        .expect("create fixture and begin a READ COMMITTED transaction");
        let before: i64 = reader
            .query_one("SELECT count(*) FROM compat_read_committed_rows", &[])
            .await
            .expect("read the first command snapshot")
            .get(0);

        // Act
        writer
            .batch_execute("INSERT INTO compat_read_committed_rows VALUES (2)")
            .await
            .expect("commit a row from the other session");
        let after: i64 = reader
            .query_one("SELECT count(*) FROM compat_read_committed_rows", &[])
            .await
            .expect("read using a fresh READ COMMITTED command snapshot")
            .get(0);
        reader
            .batch_execute("ROLLBACK")
            .await
            .expect("finish the read transaction");

        // Assert
        assert_eq!((before, after), (1, 2));

        drop((reader, writer));
        server.shutdown(reader_connection).await;
        writer_connection.abort();
        let _ = writer_connection.await;
    });
}
#[test]
fn should_map_read_uncommitted_mode_to_read_committed() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("read_uncommitted_alias").await;
        let (reader, reader_connection) = server.connect().await;
        let (writer, writer_connection) = server.connect().await;
        reader
            .batch_execute(
                "CREATE TABLE compat_read_uncommitted_rows (id INT PRIMARY KEY); \
                 INSERT INTO compat_read_uncommitted_rows VALUES (1)",
            )
            .await
            .expect("create the READ UNCOMMITTED fixture");
        writer
            .batch_execute("BEGIN")
            .await
            .expect("begin the writer transaction");
        writer
            .batch_execute("INSERT INTO compat_read_uncommitted_rows VALUES (2)")
            .await
            .expect("hold an uncommitted row for the dirty-read probe");
        reader
            .batch_execute("BEGIN ISOLATION LEVEL READ UNCOMMITTED")
            .await
            .expect("accept PostgreSQL's READ UNCOMMITTED mode");

        // Act
        let isolation: String = reader
            .query_one("SHOW transaction_isolation", &[])
            .await
            .expect("read the effective isolation level")
            .get(0);
        let before_commit: i64 = reader
            .query_one("SELECT count(*) FROM compat_read_uncommitted_rows", &[])
            .await
            .expect("do not read the writer's uncommitted row")
            .get(0);
        writer
            .batch_execute("COMMIT")
            .await
            .expect("commit the row from the writer session");
        let after_commit: i64 = reader
            .query_one("SELECT count(*) FROM compat_read_uncommitted_rows", &[])
            .await
            .expect("take a new READ UNCOMMITTED command snapshot")
            .get(0);
        reader
            .batch_execute("ROLLBACK")
            .await
            .expect("finish the transaction");

        // Assert
        assert_eq!(isolation, "read uncommitted");
        assert_eq!((before_commit, after_commit), (1, 2));

        drop((reader, writer));
        server.shutdown(reader_connection).await;
        writer_connection.abort();
        let _ = writer_connection.await;
    });
}
struct UniqueKeyWaitObservation {
    request_entered_engine: bool,
    completed_before_commit: bool,
    owner_commit: Result<(), tokio_postgres::Error>,
    contender_result: Result<u64, tokio_postgres::Error>,
}

async fn observe_unique_key_owner_wait() -> UniqueKeyWaitObservation {
    support::use_local_storage();
    let path = support::data_dir("unique_key_transaction_wait");
    let _directory = DataDirectory(path.clone());
    let crate::support_unique_conflict::Fixture {
        cassie,
        owner,
        contender,
        owner_connection,
        contender_connection,
        server,
    } = crate::support_unique_conflict::start(&path).await;
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
    let owner_wait =
        crate::support_unique_conflict::observe_owner_wait(&cassie, &mut contender_insert).await;
    let request_entered_engine = owner_wait.request_entered_engine;
    let early_completion = owner_wait.early_completion;
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

    drop(owner);
    contender_connection.abort();
    let _ = contender_connection.await;
    owner_connection.abort();
    let _ = owner_connection.await;
    server.abort();
    let _ = server.await;
    UniqueKeyWaitObservation {
        request_entered_engine,
        completed_before_commit,
        owner_commit,
        contender_result,
    }
}

#[test]
fn should_wait_for_unique_key_owner_before_returning_pgwire_insert() {
    // Arrange
    let runtime = current_thread_runtime();

    // Act
    let observation = runtime.block_on(observe_unique_key_owner_wait());

    // Assert
    assert!(
        !observation.completed_before_commit,
        "PostgreSQL waits for the in-flight pgwire INSERT before owner resolution; \
         engine entry observed: {}; completed before owner COMMIT: {}",
        observation.request_entered_engine, observation.completed_before_commit
    );
    observation
        .owner_commit
        .expect("the transaction that inserted the unique value commits");
    let error = observation
        .contender_result
        .expect_err("the committed owner keeps the unique value");
    assert_eq!(db_error(&error).code().code(), "23505");
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
