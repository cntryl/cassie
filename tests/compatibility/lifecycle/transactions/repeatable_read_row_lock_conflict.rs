use super::*;

#[test]
fn should_abort_repeatable_read_row_lock_after_concurrent_update() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("repeatable_read_row_lock_conflict").await;
        let (reader, reader_connection) = server.connect().await;
        let (writer, writer_connection) = server.connect().await;
        execute_statements(
            &reader,
            &[
                "CREATE TABLE compat_repeatable_read_lock_conflict \
                   (id INT PRIMARY KEY, value TEXT NOT NULL)",
                "INSERT INTO compat_repeatable_read_lock_conflict VALUES (1, 'initial')",
                "BEGIN ISOLATION LEVEL REPEATABLE READ",
            ],
        )
        .await
        .expect("create the row and begin the repeatable-read snapshot");
        let snapshot_value: String = reader
            .query_one(
                "SELECT value FROM compat_repeatable_read_lock_conflict WHERE id = 1",
                &[],
            )
            .await
            .expect("establish the reader's snapshot before the concurrent update")
            .get(0);

        // Act
        writer
            .execute(
                "UPDATE compat_repeatable_read_lock_conflict SET value = 'writer' WHERE id = 1",
                &[],
            )
            .await
            .expect("commit a concurrent update in the writer session");
        let lock_conflict = reader
            .query_one(
                "SELECT value FROM compat_repeatable_read_lock_conflict \
                 WHERE id = 1 FOR UPDATE",
                &[],
            )
            .await;
        let aborted_followup = reader.query_one("SELECT 1", &[]).await;
        reader
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back after PostgreSQL rejects locking the changed row");
        let recovered_value: String = reader
            .query_one(
                "SELECT value FROM compat_repeatable_read_lock_conflict WHERE id = 1",
                &[],
            )
            .await
            .expect("read the committed row after recovering the session")
            .get(0);

        // Assert
        assert_eq!(snapshot_value, "initial");
        assert_eq!(
            db_error(
                &lock_conflict
                    .expect_err("REPEATABLE READ cannot lock a row changed after its snapshot",)
            )
            .code()
            .code(),
            "40001"
        );
        assert_eq!(
            db_error(
                &aborted_followup
                    .expect_err("a serialization failure leaves the explicit transaction aborted",)
            )
            .code()
            .code(),
            "25P02"
        );
        assert_eq!(recovered_value, "writer");

        drop((reader, writer));
        server.shutdown(reader_connection).await;
        writer_connection.abort();
        let _ = writer_connection.await;
    });
}
