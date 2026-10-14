use super::*;

#[derive(Clone, Copy)]
enum RowOperation {
    Insert,
    SelectForUpdate,
}

fn assert_table_lock_blocks_row_operation(
    server_name: &'static str,
    table_mode: &'static str,
    operation: RowOperation,
) {
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start(server_name).await;
        let (owner, owner_connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        owner
            .batch_execute(
                "CREATE TABLE compat_table_row_operation (id INT PRIMARY KEY); \
                 INSERT INTO compat_table_row_operation VALUES (1)",
            )
            .await
            .expect("create and seed the table before acquiring the table lock");
        contender
            .batch_execute("SET lock_timeout = '100ms'")
            .await
            .expect("bound any unexpected wait");
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin the table-lock owner transaction");
        owner
            .batch_execute(&format!(
                "LOCK TABLE compat_table_row_operation IN {table_mode} MODE"
            ))
            .await
            .expect("acquire the table lock before the row operation");
        contender
            .batch_execute("BEGIN")
            .await
            .expect("begin the row-operation contender transaction");

        // Act
        let operation_sql = match operation {
            RowOperation::Insert => "INSERT INTO compat_table_row_operation VALUES (2)",
            RowOperation::SelectForUpdate => {
                "SELECT id FROM compat_table_row_operation WHERE id = 1 FOR UPDATE"
            }
        };
        let attempt = match operation {
            RowOperation::Insert => contender.execute(operation_sql, &[]).await.map(|_| ()),
            RowOperation::SelectForUpdate => {
                contender.query_one(operation_sql, &[]).await.map(|_| ())
            }
        };
        let conflict = attempt.expect_err("the held table mode must conflict with this operation");
        let aborted_followup = contender.query_one("SELECT 1", &[]).await;
        contender
            .batch_execute("ROLLBACK")
            .await
            .expect("recover the contender after the lock timeout");
        owner
            .batch_execute("COMMIT")
            .await
            .expect("release the conflicting table lock");
        contender
            .batch_execute("RESET lock_timeout")
            .await
            .expect("restore the contender lock timeout");
        let retry_succeeded = match operation {
            RowOperation::Insert => contender
                .execute(operation_sql, &[])
                .await
                .map(|affected| affected == 1),
            RowOperation::SelectForUpdate => contender
                .query_one(operation_sql, &[])
                .await
                .map(|row| row.get::<_, i32>(0) == 1),
        }
        .expect("the row operation should proceed after the table lock is released");
        let row_count: i64 = contender
            .query_one("SELECT count(*) FROM compat_table_row_operation", &[])
            .await
            .expect("read table rows after the recovered operation")
            .get(0);

        // Assert
        assert_eq!(
            db_error(&conflict).code().code(),
            "55P03",
            "wrong SQLSTATE for table mode {table_mode} and operation {operation_sql}"
        );
        assert_eq!(
            db_error(&aborted_followup.expect_err("lock timeout aborts the explicit transaction"))
                .code()
                .code(),
            "25P02"
        );
        assert!(retry_succeeded);
        assert_eq!(
            row_count,
            if matches!(operation, RowOperation::Insert) {
                2
            } else {
                1
            }
        );

        drop((owner, contender));
        server.shutdown(owner_connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
    });
}

// The parent suite already checks ACCESS SHARE and ACCESS EXCLUSIVE lock conflict behavior.
#[test]
fn should_accept_the_other_postgresql_table_lock_modes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("table_lock_mode_contracts").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_table_lock_modes (id INT)")
            .await
            .expect("create the table lock fixture");

        let modes = [
            "ROW SHARE",
            "ROW EXCLUSIVE",
            "SHARE UPDATE EXCLUSIVE",
            "SHARE",
            "SHARE ROW EXCLUSIVE",
            "EXCLUSIVE",
        ];

        // Act
        let mut failures = Vec::new();
        for mode in modes {
            let begin = client.batch_execute("BEGIN").await;
            let lock = if begin.is_ok() {
                client
                    .batch_execute(&format!(
                        "LOCK TABLE compat_table_lock_modes IN {mode} MODE"
                    ))
                    .await
            } else {
                begin
            };
            if let Err(error) = lock {
                failures.push((
                    mode,
                    db_error(&error).code().code().to_string(),
                    error.to_string(),
                ));
            }
            let rollback = client.batch_execute("ROLLBACK").await;
            if let Err(error) = rollback {
                failures.push((
                    mode,
                    db_error(&error).code().code().to_string(),
                    format!("ROLLBACK after lock attempt: {error}"),
                ));
            }
        }

        // Assert
        assert!(
            failures.is_empty(),
            "PostgreSQL 18.6 accepts the remaining table lock modes; Cassie failures: {failures:?}"
        );
        drop(client);
        connection.abort();
        let _ = connection.await;
    });
}

#[test]
fn should_enforce_postgresql_table_lock_conflict_matrix_across_sessions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("table_lock_conflict_matrix").await;
        let (owner, owner_connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        owner
            .batch_execute("CREATE TABLE compat_table_lock_matrix (id INT)")
            .await
            .expect("create the table lock matrix fixture");
        contender
            .batch_execute("SET statement_timeout = '1s'")
            .await
            .expect("bound any unexpected wait caused by missing NOWAIT behavior");

        // PostgreSQL 18.6 table-lock conflicts, in ACCESS SHARE through ACCESS EXCLUSIVE order.
        let modes = [
            "ACCESS SHARE",
            "ROW SHARE",
            "ROW EXCLUSIVE",
            "SHARE UPDATE EXCLUSIVE",
            "SHARE",
            "SHARE ROW EXCLUSIVE",
            "EXCLUSIVE",
            "ACCESS EXCLUSIVE",
        ];
        let conflicts = [
            [false, false, false, false, false, false, false, true],
            [false, false, false, false, false, false, true, true],
            [false, false, false, false, true, true, true, true],
            [false, false, false, true, true, true, true, true],
            [false, false, true, true, false, true, true, true],
            [false, false, true, true, true, true, true, true],
            [false, true, true, true, true, true, true, true],
            [true, true, true, true, true, true, true, true],
        ];

        for (owner_mode, expected_conflicts) in modes.iter().zip(conflicts) {
            for (contender_mode, expected_conflict) in modes.iter().zip(expected_conflicts) {
                owner
                    .batch_execute("BEGIN")
                    .await
                    .expect("begin the lock-owning transaction");
                owner
                    .batch_execute(&format!(
                        "LOCK TABLE compat_table_lock_matrix IN {owner_mode} MODE"
                    ))
                    .await
                    .unwrap_or_else(|error| {
                        panic!("PostgreSQL accepts owner table lock mode {owner_mode}: {error}")
                    });
                contender
                    .batch_execute("BEGIN")
                    .await
                    .expect("begin the competing transaction");

                // Act
                let contender_lock = contender
                    .batch_execute(&format!(
                        "LOCK TABLE compat_table_lock_matrix IN {contender_mode} MODE NOWAIT"
                    ))
                    .await;

                // Assert
                if expected_conflict {
                    let error = match contender_lock {
                        Err(error) => error,
                        Ok(()) => panic!(
                            "PostgreSQL 18.6 expects {owner_mode} to conflict with {contender_mode}"
                        ),
                    };
                    assert_eq!(
                        db_error(&error).code().code(),
                        "55P03",
                        "wrong SQLSTATE for {owner_mode} versus {contender_mode}: {error}"
                    );
                } else {
                    if let Err(error) = contender_lock {
                        panic!(
                            "PostgreSQL 18.6 expects {owner_mode} to be compatible with {contender_mode}: {error}"
                        );
                    }
                }

                contender
                    .batch_execute("ROLLBACK")
                    .await
                    .expect("release the contender transaction");
                owner
                    .batch_execute("ROLLBACK")
                    .await
                    .expect("release the owner transaction");
            }
        }

        drop((owner, contender));
        server.shutdown(owner_connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
    });
}

#[test]
fn should_block_row_inserts_under_conflicting_table_lock_modes() {
    // Arrange
    let cases = [
        ("table_share_blocks_insert", "SHARE"),
        (
            "table_share_row_exclusive_blocks_insert",
            "SHARE ROW EXCLUSIVE",
        ),
        ("table_exclusive_blocks_insert", "EXCLUSIVE"),
        ("table_access_exclusive_blocks_insert", "ACCESS EXCLUSIVE"),
    ];

    // Act
    for (server_name, mode) in cases {
        assert_table_lock_blocks_row_operation(server_name, mode, RowOperation::Insert);
    }

    // Assert
    // The helper checks 55P03, 25P02, rollback, recovery, and insertion after release.
}

#[test]
fn should_block_for_update_under_exclusive_table_lock_modes() {
    // Arrange
    let cases = [
        ("table_exclusive_blocks_for_update", "EXCLUSIVE"),
        (
            "table_access_exclusive_blocks_for_update",
            "ACCESS EXCLUSIVE",
        ),
    ];

    // Act
    for (server_name, mode) in cases {
        assert_table_lock_blocks_row_operation(server_name, mode, RowOperation::SelectForUpdate);
    }

    // Assert
    // The helper checks 55P03, 25P02, rollback, recovery, and the selected row after release.
}
