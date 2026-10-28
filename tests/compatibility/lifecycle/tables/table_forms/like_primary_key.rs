use super::*;

#[test]
fn should_publish_target_owned_composite_key_clone_at_commit() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_composite_primary_key").await;
        let (client, connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        execute_statements(&client, &[
            "CREATE SCHEMA clone_scope",
            "CREATE TABLE clone_scope.source(a INT, b INT, CONSTRAINT original_key PRIMARY KEY(b, a))",
            "INSERT INTO clone_scope.source VALUES(1, 2)",
            "ALTER INDEX clone_scope.original_key RENAME TO renamed_source_key",
            "BEGIN",
        ]).await.unwrap();
        let source_oid: u32 = observer.query_one("SELECT oid FROM pg_class WHERE relname = 'renamed_source_key'", &[]).await.unwrap().get(0);
        // Act
        client.batch_execute("CREATE TABLE clone_scope.target(LIKE clone_scope.source INCLUDING ALL)").await.unwrap();
        let hidden = observer.query("SELECT oid FROM pg_class WHERE relname = 'target_pkey'", &[]).await.unwrap();
        client.batch_execute("INSERT INTO clone_scope.target VALUES(1, 2), (1, 3)").await.unwrap();
        client.batch_execute("COMMIT").await.unwrap();
        // Assert
        assert_eq!(hidden.len(), 0);
        let metadata = observer.query_one("SELECT oid FROM pg_class WHERE relname = 'target_pkey'", &[]).await.unwrap();
        assert_ne!(metadata.get::<_, u32>(0), source_oid);
        let index = observer.query("SELECT indexname, indexdef FROM pg_indexes WHERE schemaname = 'clone_scope' AND tablename = 'target'", &[]).await.unwrap();
        assert_eq!(index.len(), 1);
        assert_eq!(index[0].get::<_, String>(0), "target_pkey");
        assert!(index[0].get::<_, String>(1).contains("(b, a)"));
        let duplicate = client.batch_execute("INSERT INTO clone_scope.target VALUES(1, 2)").await.unwrap_err();
        assert_eq!(db_error(&duplicate).code().code(), "23505");
        assert_eq!(db_error(&duplicate).constraint(), Some("target_pkey"));
        drop(observer);
        drop(client);
        server.shutdown(connection).await;
        observer_connection.abort();
        let _ = observer_connection.await;
    });
}
