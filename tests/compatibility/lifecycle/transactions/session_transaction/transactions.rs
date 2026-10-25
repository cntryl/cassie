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
