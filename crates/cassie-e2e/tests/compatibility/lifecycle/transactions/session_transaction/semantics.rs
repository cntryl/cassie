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
        execute_statements(
            &reader,
            &[
                "CREATE TABLE compat_read_uncommitted_rows (id INT PRIMARY KEY)",
                "INSERT INTO compat_read_uncommitted_rows VALUES (1)",
            ],
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
    let crate::support_unique_conflict::Fixture {
        cassie,
        owner,
        contender,
        owner_connection,
        contender_connection,
        server,
    } = crate::support_unique_conflict::start().await;
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
