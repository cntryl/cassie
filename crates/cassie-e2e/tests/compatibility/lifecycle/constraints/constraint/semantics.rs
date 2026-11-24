#[test]
fn should_protect_constraint_owned_index_until_constraint_drop() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("constraint_index_dependency_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_constraint_index_dependency (id INT, payload TEXT)",
                "CREATE UNIQUE INDEX compat_constraint_index_dependency_index \
                   ON compat_constraint_index_dependency (id)",
            ],
        )
        .await
        .expect("create the table and unique index to attach");
        let original_index_oid: i64 = client
            .query_one(
                "SELECT indexrelid_oid::BIGINT FROM pg_catalog.pg_index \
                 WHERE indexrelid = 'compat_constraint_index_dependency_index'",
                &[],
            )
            .await
            .expect("read the existing index relation identity")
            .get(0);
        client
            .batch_execute(
                "ALTER TABLE compat_constraint_index_dependency \
                 ADD CONSTRAINT compat_constraint_index_dependency_key \
                 UNIQUE USING INDEX compat_constraint_index_dependency_index",
            )
            .await
            .expect("attach the unique index to a named table constraint");

        // Act
        let rejected_index_drop = client
            .batch_execute("DROP INDEX compat_constraint_index_dependency_key")
            .await
            .expect_err("the backing index must remain owned by its constraint");
        let dependency = client
            .query_one(
                "SELECT constraint_row.conindid = index_row.indexrelid, \
                        index_class.relname, index_row.indexrelid_oid::BIGINT \
                 FROM pg_catalog.pg_constraint AS constraint_row \
                 JOIN pg_catalog.pg_index AS index_row \
                   ON index_row.indexrelid = constraint_row.conindid \
                 JOIN pg_catalog.pg_class AS index_class \
                   ON index_class.oid = index_row.indexrelid_oid \
                 WHERE constraint_row.conname = \
                   'compat_constraint_index_dependency_key'",
                &[],
            )
            .await
            .expect("read the retained constraint-to-index dependency");
        client
            .batch_execute(
                "ALTER TABLE compat_constraint_index_dependency \
                 DROP CONSTRAINT compat_constraint_index_dependency_key",
            )
            .await
            .expect("drop the named constraint and its owned index together");
        let remaining = client
            .query_one(
                "SELECT (SELECT count(*) FROM pg_catalog.pg_constraint \
                           WHERE conname = 'compat_constraint_index_dependency_key'), \
                        (SELECT count(*) FROM pg_catalog.pg_class \
                           WHERE relname = 'compat_constraint_index_dependency_key')",
                &[],
            )
            .await
            .expect("check constraint and backing-index catalog cleanup");
        let duplicate_rows = client
            .execute(
                "INSERT INTO compat_constraint_index_dependency VALUES \
                 (1, 'first'), (1, 'second')",
                &[],
            )
            .await
            .expect("allow duplicates after dropping their owning constraint");

        // Assert
        assert_eq!(db_error(&rejected_index_drop).code().code(), "2BP01");
        assert!(dependency.get::<_, bool>(0));
        assert_eq!(
            dependency.get::<_, String>(1),
            "compat_constraint_index_dependency_key"
        );
        assert_eq!(dependency.get::<_, i64>(2), original_index_oid);
        assert_eq!(remaining.get::<_, i64>(0), 0);
        assert_eq!(remaining.get::<_, i64>(1), 0);
        assert_eq!(duplicate_rows, 2);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_enforce_check_on_upsert_update_without_changing_existing_row() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("check_constraint_upsert_update").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_check_upsert (id INT PRIMARY KEY, amount INT, \
                   CONSTRAINT compat_check_upsert_nonnegative CHECK (amount >= 0))",
                "INSERT INTO compat_check_upsert VALUES (1, 10)",
            ],
        )
        .await
        .expect("create the constrained upsert target and an existing row");

        // Act
        let rejected_upsert = client
            .execute(
                "INSERT INTO compat_check_upsert VALUES (1, -5) \
                 ON CONFLICT (id) DO UPDATE SET amount = EXCLUDED.amount",
                &[],
            )
            .await
            .expect_err("the conflict-update row must satisfy its CHECK constraint");
        let rows = client
            .query(
                "SELECT id, amount FROM compat_check_upsert ORDER BY id",
                &[],
            )
            .await
            .expect("read the original row after rejected upsert");

        // Assert
        assert_eq!(db_error(&rejected_upsert).code().code(), "23514");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, i32>(1), 10);

        drop(client);
        server.shutdown(connection).await;
    });
}
