use super::*;

#[test]
fn should_resolve_target_alias_in_conditional_conflict_update() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("conditional_upsert_target_alias").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_upsert_alias (\
                   id INT PRIMARY KEY, label TEXT, version INT)",
                "INSERT INTO compat_upsert_alias VALUES (1, 'old', 2)",
            ],
        )
        .await
        .expect("create target-alias upsert fixture and seed the conflicting row");

        let upsert = "INSERT INTO compat_upsert_alias AS target (id, label, version) \
                      VALUES ($1, $2, $3) \
                      ON CONFLICT (id) DO UPDATE \
                      SET label = EXCLUDED.label, version = EXCLUDED.version \
                      WHERE target.version < EXCLUDED.version \
                      RETURNING target.id, target.label, target.version";

        // Act
        let updated = client
            .query(upsert, &[&1_i32, &"new", &3_i32])
            .await
            .expect("resolve the INSERT target alias in the conflict predicate and RETURNING");
        let skipped = client
            .query(upsert, &[&1_i32, &"stale", &1_i32])
            .await
            .expect("skip a stale parameterized conflict update");
        let stored = client
            .query_one(
                "SELECT id, label, version FROM compat_upsert_alias WHERE id = 1",
                &[],
            )
            .await
            .expect("read the target after both conditional conflict attempts");

        // Assert
        assert_eq!(updated.len(), 1);
        assert_eq!(
            updated[0]
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![23, 25, 23]
        );
        assert_eq!(updated[0].get::<_, i32>(0), 1);
        assert_eq!(updated[0].get::<_, String>(1), "new");
        assert_eq!(updated[0].get::<_, i32>(2), 3);
        assert!(skipped.is_empty(), "a false DO UPDATE WHERE returns no row");
        assert_eq!(stored.get::<_, i32>(0), 1);
        assert_eq!(stored.get::<_, String>(1), "new");
        assert_eq!(stored.get::<_, i32>(2), 3);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_assign_multiple_conflict_columns_from_a_row_constructor() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("upsert_row_constructor_assignment").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_upsert_row_constructor (\
                   id INT PRIMARY KEY, label TEXT, version INT)",
                "INSERT INTO compat_upsert_row_constructor VALUES (1, 'old', 1)",
            ],
        )
        .await
        .expect("create the tuple-assignment upsert fixture");

        // Act
        let updated = client
            .query(
                "INSERT INTO compat_upsert_row_constructor AS target (id, label, version) \
                 VALUES (1, 'incoming', 7) \
                 ON CONFLICT (id) DO UPDATE \
                 SET (label, version) = ROW(EXCLUDED.label, EXCLUDED.version) \
                 RETURNING target.id, target.label, target.version",
                &[],
            )
            .await
            .expect("assign two conflict columns from a PostgreSQL ROW constructor");

        // Assert
        assert_eq!(updated.len(), 1);
        assert_eq!(
            updated[0]
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![23, 25, 23]
        );
        assert_eq!(updated[0].get::<_, i32>(0), 1);
        assert_eq!(updated[0].get::<_, String>(1), "incoming");
        assert_eq!(updated[0].get::<_, i32>(2), 7);

        drop(client);
        server.shutdown(connection).await;
    });
}
