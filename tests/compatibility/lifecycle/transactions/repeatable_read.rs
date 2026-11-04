//! Selected REPEATABLE READ first-query view retention for issue #819.

use super::*;

#[test]
fn should_retain_repeatable_read_view_from_the_first_query() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("repeatable_read_first_query").await;
        let (reader, reader_connection) = server.connect().await;
        let (writer, writer_connection) = server.connect().await;
        execute_statements(
            &writer,
            &[
                "CREATE TABLE rr_first_query_rows (id INT PRIMARY KEY)",
                "INSERT INTO rr_first_query_rows VALUES (1)",
            ],
        )
        .await
        .expect("seed two-session snapshot fixture");
        reader
            .batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .await
            .expect("begin selected read-only REPEATABLE READ");
        let isolation: String = reader
            .query_one("SHOW transaction_isolation", &[])
            .await
            .expect("SHOW does not acquire a data snapshot")
            .get(0);
        reader.batch_execute("SAVEPOINT before_view").await.expect("savepoint");

        // Act
        writer
            .batch_execute("INSERT INTO rr_first_query_rows VALUES (2)")
            .await
            .expect("commit before the first snapshot-taking statement");
        let first: i64 = reader
            .query_one("SELECT count(*) FROM rr_first_query_rows", &[])
            .await
            .expect("acquire first transaction view")
            .get(0);
        execute_statements(
            &writer,
            &[
                "INSERT INTO rr_first_query_rows VALUES (3)",
                "CREATE TABLE rr_created_after_view (id INT PRIMARY KEY)",
            ],
        )
        .await
        .expect("commit data and catalog changes after view capture");
        reader
            .batch_execute("ROLLBACK TO SAVEPOINT before_view")
            .await
            .expect("savepoint rollback preserves the retained transaction view");
        let retained: i64 = reader
            .query_one("SELECT count(*) FROM rr_first_query_rows", &[])
            .await
            .expect("reuse the transaction data view")
            .get(0);
        let catalog: i64 = reader
            .query_one(
                "SELECT count(*) FROM information_schema.tables WHERE table_name = 'rr_created_after_view'",
                &[],
            )
            .await
            .expect("SQL catalog queries reuse the transaction view")
            .get(0);
        let current_rows = reader
            .query("SELECT id FROM rr_created_after_view", &[])
            .await
            .expect("current row encoding descriptor resolves the newly created table");
        let current_descriptor: i64 = reader
            .query_one("SELECT count(*) FROM rr_created_after_view", &[])
            .await
            .expect("current descriptors resolve the newly created table")
            .get(0);
        reader.batch_execute("COMMIT").await.expect("finish snapshot");
        let fresh: i64 = reader
            .query_one("SELECT count(*) FROM rr_first_query_rows", &[])
            .await
            .expect("fresh transaction sees later rows")
            .get(0);
        let fresh_catalog: i64 = reader
            .query_one(
                "SELECT count(*) FROM information_schema.tables WHERE table_name = 'rr_created_after_view'",
                &[],
            )
            .await
            .expect("fresh transaction sees later catalog rows")
            .get(0);

        // Assert
        assert_eq!(isolation, "repeatable read");
        assert_eq!((first, retained, fresh), (2, 2, 3));
        assert_eq!((catalog, current_descriptor, fresh_catalog), (0, 0, 1));
        assert!(current_rows.is_empty());
        drop((reader, writer));
        server.shutdown(reader_connection).await;
        writer_connection.abort();
        let _ = writer_connection.await;
    });
}

#[test]
fn should_read_historical_rows_through_current_repeatable_read_descriptors() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("repeatable_read_current_descriptors").await;
        let (reader, reader_connection) = server.connect().await;
        let (writer, writer_connection) = server.connect().await;
        execute_statements(&writer, &[
            "CREATE TABLE rr_descriptor_rows (id INT PRIMARY KEY)",
            "INSERT INTO rr_descriptor_rows VALUES (1)",
        ]).await.expect("seed historical row");
        reader.batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY; SELECT 1")
            .await.expect("capture before taking a relation lock");

        // Act
        execute_statements(&writer, &[
            "ALTER TABLE rr_descriptor_rows RENAME COLUMN id TO renamed",
            "ALTER TABLE rr_descriptor_rows ADD COLUMN note TEXT",
        ]).await.expect("publish current descriptors after snapshot");
        let row = reader.query_one("SELECT renamed, note FROM rr_descriptor_rows", &[])
            .await.expect("current descriptors decode historical row");
        let old_catalog = reader.query("SELECT column_name FROM information_schema.columns WHERE table_name = 'rr_descriptor_rows' ORDER BY ordinal_position", &[])
            .await.expect("SQL metadata retains its original object rows");
        reader.batch_execute("ROLLBACK").await.expect("release view");

        // Assert
        assert_eq!(row.get::<_, i32>(0), 1);
        assert_eq!(row.get::<_, Option<String>>(1), None);
        assert_eq!(old_catalog.iter().map(|row| row.get::<_, String>(0)).collect::<Vec<_>>(), vec!["id"]);
        drop((reader, writer));
        server.shutdown(reader_connection).await;
        writer_connection.abort();
        let _ = writer_connection.await;
    });
}
