use super::*;

#[test]
fn should_publish_quoted_index_rename_at_transaction_commit() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("index_rename_visibility").await;
        let (client, connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA rename_scope",
                "CREATE TABLE rename_scope.rows(value INT)",
                "INSERT INTO rename_scope.rows VALUES(7)",
                "CREATE UNIQUE INDEX old_idx ON rename_scope.rows(value)",
                "BEGIN",
            ],
        )
        .await
        .unwrap();
        let before = observer
            .query_one("SELECT oid FROM pg_class WHERE relname = 'old_idx'", &[])
            .await
            .unwrap()
            .get::<_, u32>(0);
        // Act
        client
            .batch_execute("ALTER INDEX rename_scope.old_idx RENAME TO \"Renamed.Index\"")
            .await
            .unwrap();
        let uncommitted = observer
            .query(
                "SELECT indexname FROM pg_indexes WHERE tablename = 'rows'",
                &[],
            )
            .await
            .unwrap();
        client.batch_execute("COMMIT").await.unwrap();
        // Assert
        assert_eq!(uncommitted.len(), 1);
        assert_eq!(uncommitted[0].get::<_, String>(0), "old_idx");
        let after = observer
            .query_one(
                "SELECT oid FROM pg_class WHERE relname = 'Renamed.Index'",
                &[],
            )
            .await
            .unwrap()
            .get::<_, u32>(0);
        assert_eq!(after, before);
        let duplicate = client
            .batch_execute("INSERT INTO rename_scope.rows VALUES(7)")
            .await
            .unwrap_err();
        assert_eq!(db_error(&duplicate).code().code(), "23505");
        assert_eq!(db_error(&duplicate).constraint(), Some("Renamed.Index"));
        client
            .batch_execute("REINDEX INDEX rename_scope.\"Renamed.Index\"")
            .await
            .unwrap();
        client
            .batch_execute("DROP INDEX rename_scope.\"Renamed.Index\"")
            .await
            .unwrap();
        drop(observer);
        drop(client);
        server.shutdown(connection).await;
        observer_connection.abort();
        let _ = observer_connection.await;
    });
}

#[test]
fn should_skip_missing_index_rename_with_if_exists() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("index_rename_if_exists").await;
        let (client, connection) = server.connect().await;
        // Act
        client
            .batch_execute("ALTER INDEX IF EXISTS missing_idx RENAME TO next_idx")
            .await
            .unwrap();
        // Assert
        let rows = client
            .query(
                "SELECT indexname FROM pg_indexes WHERE indexname = 'next_idx'",
                &[],
            )
            .await
            .unwrap();
        assert_eq!(rows.len(), 0);
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_index_when_destination_constraint_name_is_occupied() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("index_rename_constraint_collision").await;
        let (client, connection) = server.connect().await;
        execute_statements(&client, &[
            "CREATE TABLE rename_parent(value INT PRIMARY KEY)",
            "CREATE TABLE rename_edge(value INT CONSTRAINT original_key UNIQUE, parent INT CONSTRAINT target_key REFERENCES rename_parent(value))",
        ]).await.unwrap();
        // Act
        let error = client.batch_execute("ALTER INDEX original_key RENAME TO target_key").await.unwrap_err();
        // Assert
        assert_eq!(db_error(&error).code().code(), "42710");
        let indexes = client.query("SELECT indexname FROM pg_indexes WHERE tablename = 'rename_edge'", &[]).await.unwrap();
        assert_eq!(indexes.len(), 1);
        assert_eq!(indexes[0].get::<_, String>(0), "original_key");
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_conflicting_transactional_index_name_publication() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("index_rename_namespace_publication").await;
        let (client, connection) = server.connect().await;
        let (competitor, competitor_connection) = server.connect().await;
        execute_statements(&client, &[
            "CREATE TABLE rename_left(value INT)", "CREATE TABLE rename_right(value INT)",
            "CREATE INDEX left_idx ON rename_left(value)", "CREATE INDEX right_idx ON rename_right(value)",
            "BEGIN", "ALTER INDEX left_idx RENAME TO shared_idx",
        ]).await.unwrap();
        execute_statements(&competitor, &["BEGIN", "ALTER INDEX right_idx RENAME TO shared_idx"]).await.unwrap();
        // Act
        client.batch_execute("COMMIT").await.unwrap();
        let conflict = competitor.batch_execute("COMMIT").await.unwrap_err();
        competitor.batch_execute("ROLLBACK").await.unwrap();
        // Assert
        assert_eq!(db_error(&conflict).code().code(), "40001");
        let indexes = client.query("SELECT indexname FROM pg_indexes WHERE tablename IN ('rename_left', 'rename_right') ORDER BY indexname", &[]).await.unwrap();
        let names = indexes.iter().map(|row| row.get::<_, String>(0)).collect::<Vec<_>>();
        assert_eq!(names, vec!["right_idx", "shared_idx"]);
        drop(competitor);
        drop(client);
        server.shutdown(connection).await;
        competitor_connection.abort();
        let _ = competitor_connection.await;
    });
}
