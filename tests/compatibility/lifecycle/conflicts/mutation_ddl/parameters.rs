#[test]
fn should_infer_partial_expression_index_for_conflict_updates() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("partial_expression_conflict_inference").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_partial_conflicts (id INT PRIMARY KEY, email TEXT, active BOOLEAN, version INT)",
                "CREATE UNIQUE INDEX compat_partial_email_active ON compat_partial_conflicts ((lower(email))) WHERE active",
                "INSERT INTO compat_partial_conflicts VALUES (1, 'A@example.com', TRUE, 1), (2, 'a@example.com', FALSE, 9)",
            ],
        )
        .await
        .expect("create partial expression index and seed active/inactive values");

        // Act
        let changed = client
            .query(
                "INSERT INTO compat_partial_conflicts AS target (id, email, active, version) \
                 VALUES (3, 'a@EXAMPLE.com', TRUE, 2) \
                 ON CONFLICT (lower(email)) WHERE active \
                 DO UPDATE SET version = EXCLUDED.version \
                 WHERE target.version < EXCLUDED.version \
                 RETURNING id, version",
                &[],
            )
            .await
            .expect("infer the partial expression index for the conflict target");
        let unchanged = client
            .query(
                "INSERT INTO compat_partial_conflicts AS target (id, email, active, version) \
                 VALUES (4, 'a@example.com', TRUE, 2) \
                 ON CONFLICT (lower(email)) WHERE active \
                 DO UPDATE SET version = EXCLUDED.version \
                 WHERE target.version < EXCLUDED.version \
                 RETURNING id, version",
                &[],
            )
            .await
            .expect("skip an update rejected by its version predicate");
        let stored = client
            .query(
                "SELECT id, email, active, version FROM compat_partial_conflicts ORDER BY active DESC, id",
                &[],
            )
            .await
            .expect("read rows after expression-index conflict handling");

        // Assert
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].get::<_, i32>(0), 1);
        assert_eq!(changed[0].get::<_, i32>(1), 2);
        assert!(unchanged.is_empty(), "a skipped update returns no row");
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].get::<_, i32>(0), 1);
        assert_eq!(stored[0].get::<_, String>(1), "A@example.com");
        assert!(stored[0].get::<_, bool>(2));
        assert_eq!(stored[0].get::<_, i32>(3), 2);
        assert_eq!(stored[1].get::<_, i32>(0), 2);
        assert!(!stored[1].get::<_, bool>(2));
        assert_eq!(stored[1].get::<_, i32>(3), 9);

        drop(client);
        server.shutdown(connection).await;
    });
}
