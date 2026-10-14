use super::*;

#[test]
fn should_apply_set_transaction_isolation_before_first_query_across_sessions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("set_transaction_repeatable_read").await;
        let (reader, reader_connection) = server.connect().await;
        let (writer, writer_connection) = server.connect().await;
        execute_statements(
            &reader,
            &[
                "CREATE TABLE compat_set_transaction_snapshot (id INT PRIMARY KEY)",
                "INSERT INTO compat_set_transaction_snapshot VALUES (1)",
                "BEGIN",
            ],
        )
        .await
        .expect("create the snapshot fixture and begin the reader transaction");

        // Act
        let isolation_set = reader
            .batch_execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .await;
        let observations = if isolation_set.is_ok() {
            let before: i64 = reader
                .query_one("SELECT count(*) FROM compat_set_transaction_snapshot", &[])
                .await
                .expect("establish the repeatable-read snapshot")
                .get(0);
            writer
                .batch_execute("INSERT INTO compat_set_transaction_snapshot VALUES (2)")
                .await
                .expect("commit a row from the independent writer session");
            let during: i64 = reader
                .query_one("SELECT count(*) FROM compat_set_transaction_snapshot", &[])
                .await
                .expect("read through the transaction's existing snapshot")
                .get(0);
            reader
                .batch_execute("COMMIT")
                .await
                .expect("finish the repeatable-read transaction");
            let after: i64 = reader
                .query_one("SELECT count(*) FROM compat_set_transaction_snapshot", &[])
                .await
                .expect("read with a fresh transaction snapshot")
                .get(0);
            Some((before, during, after))
        } else {
            reader
                .batch_execute("ROLLBACK")
                .await
                .expect("recover the reader after a rejected isolation command");
            None
        };

        // Assert
        drop((reader, writer));
        server.shutdown(reader_connection).await;
        writer_connection.abort();
        let _ = writer_connection.await;

        assert!(
            isolation_set.is_ok(),
            "SET TRANSACTION must accept REPEATABLE READ before the first query: {isolation_set:?}"
        );
        assert_eq!(
            observations.expect("repeatable-read snapshot observations"),
            (1, 1, 2)
        );
    });
}

#[test]
fn should_reject_late_set_transaction_with_sqlstate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("set_transaction_too_late").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin the transaction before its first query");
        let first_query = client
            .query_one("SELECT 1", &[])
            .await
            .expect("run a query before changing transaction isolation");

        // Act
        let late_change = client
            .batch_execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .await;
        let aborted_followup = client.query_one("SELECT 1", &[]).await;
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the failed transaction");
        let recovered: i32 = client
            .query_one("SELECT 1", &[])
            .await
            .expect("reuse the session after rollback")
            .get(0);

        // Assert
        assert_eq!(first_query.get::<_, i32>(0), 1);
        assert_eq!(recovered, 1);

        drop(client);
        server.shutdown(connection).await;

        assert_eq!(
            db_error(&late_change.expect_err("SET TRANSACTION must come before the first query"))
                .code()
                .code(),
            "25001"
        );
        assert_eq!(
            db_error(
                &aborted_followup.expect_err(
                    "the failed SET TRANSACTION leaves the explicit transaction aborted"
                )
            )
            .code()
            .code(),
            "25P02"
        );
    });
}
