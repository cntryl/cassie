#[test]
fn should_keep_sequence_allocation_after_transaction_rollback() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sequence_rollback_allocation").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SEQUENCE compat_rollback_sequence")
            .await
            .expect("create a sequence with PostgreSQL's default allocation step");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin the transaction containing nextval");
        let allocated: i64 = client
            .query_one("SELECT nextval('compat_rollback_sequence')", &[])
            .await
            .expect("allocate a sequence value inside the transaction")
            .try_get(0)
            .expect("allocated sequence value");

        // Act
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the transaction without rewinding the sequence");
        let currval_after_rollback: i64 = client
            .query_one("SELECT currval('compat_rollback_sequence')", &[])
            .await
            .expect("retain this session's currval after rollback")
            .try_get(0)
            .expect("current sequence value after rollback");
        let lastval_after_rollback: i64 = client
            .query_one("SELECT lastval()", &[])
            .await
            .expect("retain this session's lastval after rollback")
            .try_get(0)
            .expect("last sequence value after rollback");
        let nextval_after_rollback: i64 = client
            .query_one("SELECT nextval('compat_rollback_sequence')", &[])
            .await
            .expect("continue from the nontransactional sequence allocation")
            .try_get(0)
            .expect("next sequence value after rollback");

        // Assert
        assert_eq!(
            (
                allocated,
                currval_after_rollback,
                lastval_after_rollback,
                nextval_after_rollback,
            ),
            (1, 1, 1, 2),
            "PostgreSQL 18.6 does not roll back nextval allocation or session value history"
        );

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
struct SerializableWriteSkewOutcome {
    snapshots: (i64, i64),
    serialization_failure: bool,
    remaining_on_call: i64,
    diagnostics: String,
}

struct SerializableWriteSkewProbe {
    left_begin: Result<(), String>,
    right_begin: Result<(), String>,
    outcome: Option<SerializableWriteSkewOutcome>,
}

fn has_sqlstate<T>(result: &Result<T, tokio_postgres::Error>, code: &str) -> bool {
    result
        .as_ref()
        .is_err_and(|error| db_error(error).code().code() == code)
}

async fn collect_write_skew_outcome(
    left: &tokio_postgres::Client,
    right: &tokio_postgres::Client,
) -> SerializableWriteSkewOutcome {
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
    let remaining_on_call = left
        .query_one(
            "SELECT count(*) FROM compat_serializable_on_call WHERE on_call",
            &[],
        )
        .await
        .expect("inspect the committed write-skew outcome")
        .get::<_, i64>(0);
    let serialization_failure = has_sqlstate(&left_update, "40001")
        || has_sqlstate(&right_update, "40001")
        || has_sqlstate(&left_commit, "40001")
        || has_sqlstate(&right_commit, "40001");
    let diagnostics = format!(
        "left update/commit: {left_update:?}/{left_commit:?}; \
         right update/commit: {right_update:?}/{right_commit:?}"
    );
    SerializableWriteSkewOutcome {
        snapshots: (left_snapshot, right_snapshot),
        serialization_failure,
        remaining_on_call,
        diagnostics,
    }
}

async fn probe_serializable_write_skew() -> SerializableWriteSkewProbe {
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
        .await
        .map_err(|error| format!("{error:?}"));
    let right_begin = right
        .batch_execute("BEGIN ISOLATION LEVEL SERIALIZABLE")
        .await
        .map_err(|error| format!("{error:?}"));
    let outcome = if left_begin.is_ok() && right_begin.is_ok() {
        Some(collect_write_skew_outcome(&left, &right).await)
    } else {
        None
    };

    drop((left, right));
    server.shutdown(left_connection).await;
    right_connection.abort();
    let _ = right_connection.await;
    SerializableWriteSkewProbe {
        left_begin,
        right_begin,
        outcome,
    }
}

#[test]
fn should_abort_one_serializable_transaction_after_write_skew() {
    // Arrange
    let runtime = current_thread_runtime();
    let probe = runtime.block_on(probe_serializable_write_skew());

    // Act
    // The probe captures both snapshots, writes, commit outcomes, and final visibility.

    // Assert
    assert!(
        probe.left_begin.is_ok(),
        "left SERIALIZABLE transaction: {:?}",
        probe.left_begin
    );
    assert!(
        probe.right_begin.is_ok(),
        "right SERIALIZABLE transaction: {:?}",
        probe.right_begin
    );
    let outcome = probe
        .outcome
        .expect("both serializable transactions should observe a snapshot");
    assert_eq!(outcome.snapshots, (2, 2));
    assert!(
        outcome.serialization_failure,
        "one transaction must fail with SQLSTATE 40001; {}",
        outcome.diagnostics
    );
    assert_eq!(outcome.remaining_on_call, 1);
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
