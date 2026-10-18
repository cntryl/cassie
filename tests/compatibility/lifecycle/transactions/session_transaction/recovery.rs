#[test]
fn should_report_repeatable_read_update_conflict_with_recoverable_state() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("repeatable_read_update_conflict").await;
        let (reader, reader_connection) = server.connect().await;
        let (writer, writer_connection) = server.connect().await;
        execute_statements(
            &reader,
            &[
                "CREATE TABLE compat_repeatable_update_rows (id INT PRIMARY KEY, value TEXT)",
                "INSERT INTO compat_repeatable_update_rows VALUES (1, 'initial')",
                "BEGIN ISOLATION LEVEL REPEATABLE READ",
            ],
        )
        .await
        .expect("create fixture and begin a REPEATABLE READ transaction");
        let snapshot_value: String = reader
            .query_one(
                "SELECT value FROM compat_repeatable_update_rows WHERE id = 1",
                &[],
            )
            .await
            .expect("establish the repeatable-read snapshot")
            .get(0);
        writer
            .batch_execute("UPDATE compat_repeatable_update_rows SET value = 'writer' WHERE id = 1")
            .await
            .expect("commit a concurrent update to the observed row");

        // Act
        let conflict = reader
            .execute(
                "UPDATE compat_repeatable_update_rows SET value = 'reader' WHERE id = 1",
                &[],
            )
            .await
            .expect_err("REPEATABLE READ cannot update a row changed after its snapshot");
        let aborted_transaction = reader
            .query_one("SELECT 1", &[])
            .await
            .expect_err("a serialization failure leaves the transaction aborted");
        reader
            .batch_execute("ROLLBACK")
            .await
            .expect("rollback the failed transaction");
        let recovered_value: String = reader
            .query_one(
                "SELECT value FROM compat_repeatable_update_rows WHERE id = 1",
                &[],
            )
            .await
            .expect("reuse the session after rollback")
            .get(0);

        // Assert
        assert_eq!(snapshot_value, "initial");
        assert_eq!(db_error(&conflict).code().code(), "40001");
        assert_eq!(db_error(&aborted_transaction).code().code(), "25P02");
        assert_eq!(recovered_value, "writer");

        drop((reader, writer));
        server.shutdown(reader_connection).await;
        writer_connection.abort();
        let _ = writer_connection.await;
    });
}
