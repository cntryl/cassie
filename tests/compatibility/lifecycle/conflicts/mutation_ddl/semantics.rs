#[test]
fn should_return_prior_row_for_conflict_update() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("conflict_returning_old_new").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_conflict_old_new (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_conflict_old_new VALUES (1, 'first')",
            ],
        )
        .await
        .expect("create OLD/NEW conflict RETURNING fixture");

        // Act
        let updated = client
            .query(
                "INSERT INTO compat_conflict_old_new AS target (id, label) VALUES (1, 'second') \
                 ON CONFLICT (id) DO UPDATE SET label = EXCLUDED.label \
                 RETURNING WITH (OLD AS old_row, NEW AS new_row) \
                   old_row.id, old_row.label, new_row.id, new_row.label",
                &[],
            )
            .await
            .expect("return OLD and NEW on a conflict update");

        // Assert
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].get::<_, i32>(0), 1);
        assert_eq!(updated[0].get::<_, String>(1), "first");
        assert_eq!(updated[0].get::<_, i32>(2), 1);
        assert_eq!(updated[0].get::<_, String>(3), "second");

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_return_only_rows_changed_by_a_named_conditional_conflict_target() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("conditional_conflict_target").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_conflict_targets (id INT PRIMARY KEY, email TEXT, version INT, \
                   CONSTRAINT compat_conflict_email_key UNIQUE (email))",
                "INSERT INTO compat_conflict_targets VALUES (1, 'a@example.com', 1)",
            ],
        )
            .await
            .expect("create named conflict constraint and seed row");

        // Act
        let changed = client
            .query(
                "INSERT INTO compat_conflict_targets (id, email, version) \
                 VALUES (2, 'a@example.com', 2) \
                 ON CONFLICT ON CONSTRAINT compat_conflict_email_key \
                 DO UPDATE SET version = EXCLUDED.version \
                 WHERE compat_conflict_targets.version < EXCLUDED.version \
                 RETURNING id, version",
                &[],
            )
            .await
            .expect("update the row selected by named constraint");
        let unchanged = client
            .query(
                "INSERT INTO compat_conflict_targets (id, email, version) \
                 VALUES (3, 'a@example.com', 1) \
                 ON CONFLICT ON CONSTRAINT compat_conflict_email_key \
                 DO UPDATE SET version = EXCLUDED.version \
                 WHERE compat_conflict_targets.version < EXCLUDED.version \
                 RETURNING id, version",
                &[],
            )
            .await
            .expect("skip stale conflict update");
        let stored_row = client
            .query_one(
                "SELECT id, version FROM compat_conflict_targets WHERE email = 'a@example.com'",
                &[],
            )
            .await
            .expect("read target after conflict statements");
        let stored = (stored_row.get::<_, i32>(0), stored_row.get::<_, i32>(1));

        // Assert
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].get::<_, i32>(0), 1);
        assert_eq!(changed[0].get::<_, i32>(1), 2);
        assert!(unchanged.is_empty(), "a skipped UPDATE has no RETURNING row");
        assert_eq!(stored, (1, 2));

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_assign_multiple_conflict_columns_from_a_subquery() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("conflict_tuple_subquery_assignment").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_conflict_tuple_target (id INT PRIMARY KEY, label TEXT, version INT)",
                "CREATE TABLE compat_conflict_tuple_source (id INT, label TEXT, version INT)",
                "INSERT INTO compat_conflict_tuple_target VALUES (1, 'old', 1)",
                "INSERT INTO compat_conflict_tuple_source VALUES (1, 'subquery', 9)",
            ],
        )
        .await
        .expect("create tuple conflict-assignment fixtures");

        // Act
        let changed = client
            .query(
                "INSERT INTO compat_conflict_tuple_target (id, label, version) \
                 VALUES (1, 'incoming', 2) \
                 ON CONFLICT (id) DO UPDATE SET (label, version) = ( \
                   SELECT source.label, source.version \
                   FROM compat_conflict_tuple_source AS source \
                   WHERE source.id = EXCLUDED.id \
                 ) RETURNING id, label, version",
                &[],
            )
            .await
            .expect("assign an upsert target tuple from a scalar subquery");

        // Assert
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].get::<_, i32>(0), 1);
        assert_eq!(changed[0].get::<_, String>(1), "subquery");
        assert_eq!(changed[0].get::<_, i32>(2), 9);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_apply_default_to_a_conflicting_target_column() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("conflict_default_assignment").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_conflict_default (id INT PRIMARY KEY, label TEXT DEFAULT 'fallback')",
                "INSERT INTO compat_conflict_default VALUES (1, 'old')",
            ],
        )
        .await
        .expect("create conflict DEFAULT fixture");

        // Act
        let changed = client
            .query(
                "INSERT INTO compat_conflict_default (id, label) VALUES (1, 'incoming') \
                 ON CONFLICT (id) DO UPDATE SET label = DEFAULT RETURNING id, label",
                &[],
            )
            .await
            .expect("use DEFAULT in the conflict update assignment");

        // Assert
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].get::<_, i32>(0), 1);
        assert_eq!(changed[0].get::<_, String>(1), "fallback");

        drop(client);
        server.shutdown(connection).await;
    });
}
