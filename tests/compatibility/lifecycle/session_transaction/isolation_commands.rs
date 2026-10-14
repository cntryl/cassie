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

#[test]
fn should_reject_snapshot_import_after_first_query() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("late_snapshot_import").await;
        let (exporter, exporter_connection) = server.connect().await;
        let (importer, importer_connection) = server.connect().await;
        exporter
            .batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ")
            .await
            .expect("begin the transaction that exports a snapshot");
        let snapshot: String = exporter
            .query_one("SELECT pg_export_snapshot()", &[])
            .await
            .expect("export the active repeatable-read snapshot")
            .get(0);
        importer
            .batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ")
            .await
            .expect("begin a transaction eligible to import the snapshot");
        let first_query = importer
            .query_one("SELECT 1", &[])
            .await
            .expect("run the query that establishes the importer snapshot");
        let snapshot_literal = snapshot.replace('\'', "''");

        // Act
        let late_import = importer
            .batch_execute(&format!("SET TRANSACTION SNAPSHOT '{snapshot_literal}'"))
            .await;
        let failed_followup = importer.query_one("SELECT 1", &[]).await;
        importer
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back after the late snapshot import is rejected");
        exporter
            .batch_execute("COMMIT")
            .await
            .expect("finish the exporting transaction");
        let recovered: i32 = importer
            .query_one("SELECT 1", &[])
            .await
            .expect("reuse the importing session after rollback")
            .get(0);

        // Assert
        assert_eq!(first_query.get::<_, i32>(0), 1);
        assert_eq!(
            db_error(&late_import.expect_err("snapshot import must precede all queries"))
                .code()
                .code(),
            "25001"
        );
        assert_eq!(
            db_error(&failed_followup.expect_err("late import failure aborts the transaction"))
                .code()
                .code(),
            "25P02"
        );
        assert_eq!(recovered, 1);

        drop((exporter, importer));
        server.shutdown(exporter_connection).await;
        importer_connection.abort();
        let _ = importer_connection.await;
    });
}

#[test]
fn should_reject_invalid_imported_snapshot_with_postgres_sqlstate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("invalid_imported_snapshot").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ")
            .await
            .expect("begin a transaction eligible to import a repeatable-read snapshot");

        // Act
        let invalid_snapshot = client
            .batch_execute("SET TRANSACTION SNAPSHOT 'not-a-snapshot'")
            .await;
        let rollback = client.batch_execute("ROLLBACK").await;

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            db_error(
                &invalid_snapshot
                    .expect_err("PostgreSQL rejects an invalid imported snapshot identifier")
            )
            .code()
            .code(),
            "22023"
        );
        rollback.expect("roll back the transaction aborted by the invalid snapshot identifier");
    });
}

#[test]
fn should_reject_import_of_snapshot_after_exporter_commits() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("expired_exported_snapshot").await;
        let (exporter, exporter_connection) = server.connect().await;
        let (importer, importer_connection) = server.connect().await;
        exporter
            .batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ")
            .await
            .expect("begin the transaction that owns an exported snapshot");
        let snapshot: String = exporter
            .query_one("SELECT pg_export_snapshot()", &[])
            .await
            .expect("export a repeatable-read snapshot")
            .get(0);
        exporter
            .batch_execute("COMMIT")
            .await
            .expect("end the transaction that exported the snapshot");
        importer
            .batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ")
            .await
            .expect("begin a transaction eligible to import a snapshot");
        let snapshot_literal = snapshot.replace('\'', "''");

        // Act
        let import = importer
            .batch_execute(&format!("SET TRANSACTION SNAPSHOT '{snapshot_literal}'"))
            .await;
        importer
            .batch_execute("ROLLBACK")
            .await
            .expect("recover the importer after its expired snapshot is rejected");
        let recovered: i32 = importer
            .query_one("SELECT 1", &[])
            .await
            .expect("reuse the importing session after rollback")
            .get(0);

        // Assert
        assert_eq!(
            db_error(&import.expect_err("a committed exporter invalidates new imports"))
                .code()
                .code(),
            "42704"
        );
        assert_eq!(recovered, 1);

        drop((exporter, importer));
        server.shutdown(exporter_connection).await;
        importer_connection.abort();
        let _ = importer_connection.await;
    });
}

#[test]
fn should_import_exported_repeatable_read_snapshot_across_sessions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("export_import_snapshot_visibility").await;
        let (exporter, exporter_connection) = server.connect().await;
        let (writer, writer_connection) = server.connect().await;
        let (importer, importer_connection) = server.connect().await;
        execute_statements(
            &exporter,
            &[
                "CREATE TABLE compat_export_import_snapshot (id INT PRIMARY KEY)",
                "INSERT INTO compat_export_import_snapshot VALUES (1)",
                "BEGIN ISOLATION LEVEL REPEATABLE READ",
            ],
        )
        .await
        .expect("create the shared table and begin the exporting snapshot");
        let initial_ids = exporter
            .query(
                "SELECT id FROM compat_export_import_snapshot ORDER BY id",
                &[],
            )
            .await
            .expect("establish the repeatable-read snapshot before export")
            .iter()
            .map(|row| row.get::<_, i32>(0))
            .collect::<Vec<_>>();

        // Act
        let snapshot: String = exporter
            .query_one("SELECT pg_export_snapshot()", &[])
            .await
            .expect("export the active repeatable-read snapshot")
            .get(0);
        let inserted = writer
            .execute("INSERT INTO compat_export_import_snapshot VALUES (2)", &[])
            .await
            .expect("commit a row after the exported snapshot was established");
        importer
            .batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ")
            .await
            .expect("begin a transaction eligible to import the exported snapshot");
        let snapshot_literal = snapshot.replace('\'', "''");
        importer
            .batch_execute(&format!("SET TRANSACTION SNAPSHOT '{snapshot_literal}'"))
            .await
            .expect("import the snapshot before the transaction runs a query");
        let imported_ids = importer
            .query(
                "SELECT id FROM compat_export_import_snapshot ORDER BY id",
                &[],
            )
            .await
            .expect("read through the imported snapshot")
            .iter()
            .map(|row| row.get::<_, i32>(0))
            .collect::<Vec<_>>();
        exporter
            .batch_execute("COMMIT")
            .await
            .expect("finish the exporting transaction after import");
        let imported_ids_after_exporter_commit = importer
            .query(
                "SELECT id FROM compat_export_import_snapshot ORDER BY id",
                &[],
            )
            .await
            .expect("retain the imported snapshot after its exporter commits")
            .iter()
            .map(|row| row.get::<_, i32>(0))
            .collect::<Vec<_>>();
        importer
            .batch_execute("COMMIT")
            .await
            .expect("finish the importing transaction");
        let fresh_ids = importer
            .query(
                "SELECT id FROM compat_export_import_snapshot ORDER BY id",
                &[],
            )
            .await
            .expect("read through a fresh transaction snapshot")
            .iter()
            .map(|row| row.get::<_, i32>(0))
            .collect::<Vec<_>>();

        // Assert
        assert!(!snapshot.is_empty());
        assert_eq!(inserted, 1);
        assert_eq!(initial_ids, vec![1]);
        assert_eq!(imported_ids, vec![1]);
        assert_eq!(imported_ids_after_exporter_commit, vec![1]);
        assert_eq!(fresh_ids, vec![1, 2]);

        drop((exporter, writer, importer));
        server.shutdown(exporter_connection).await;
        writer_connection.abort();
        let _ = writer_connection.await;
        importer_connection.abort();
        let _ = importer_connection.await;
    });
}

#[test]
fn should_reject_row_locking_query_in_read_only_transaction() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("read_only_repeatable_read_row_lock").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_read_only_row_lock (id INT PRIMARY KEY)",
                "INSERT INTO compat_read_only_row_lock VALUES (1)",
                "BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY",
            ],
        )
        .await
        .expect("create the row and begin a read-only repeatable-read transaction");

        // Act
        let rejected = client
            .query_one(
                "SELECT id FROM compat_read_only_row_lock WHERE id = 1 FOR UPDATE",
                &[],
            )
            .await;
        let aborted_followup = client.query_one("SELECT 1", &[]).await;
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back after the read-only row-locking query is rejected");
        let recovered_count: i64 = client
            .query_one("SELECT count(*) FROM compat_read_only_row_lock", &[])
            .await
            .expect("reuse the session after rollback")
            .get(0);

        // Assert
        assert_eq!(
            db_error(&rejected.expect_err("read-only transactions reject row locking"))
                .code()
                .code(),
            "25006"
        );
        assert_eq!(
            db_error(&aborted_followup.expect_err("the rejected statement aborts its transaction"))
                .code()
                .code(),
            "25P02"
        );
        assert_eq!(recovered_count, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
