#[test]
fn should_apply_match_simple_on_update_cascade_to_composite_keys() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("match_simple_update_cascade").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_match_simple_update_parent (\
                   left_id INT, right_id INT, PRIMARY KEY (left_id, right_id))",
                "CREATE TABLE compat_match_simple_update_child (\
                   id INT PRIMARY KEY, left_id INT, right_id INT, \
                   CONSTRAINT compat_match_simple_update_fk \
                   FOREIGN KEY (left_id, right_id) \
                   REFERENCES compat_match_simple_update_parent (left_id, right_id) \
                   MATCH SIMPLE ON UPDATE CASCADE)",
                "INSERT INTO compat_match_simple_update_parent VALUES (1, 2)",
                "INSERT INTO compat_match_simple_update_child VALUES \
                   (1, 1, NULL), (2, NULL, 1), (3, 1, 2)",
            ],
        )
        .await
        .expect("create MATCH SIMPLE foreign key and accept partially NULL keys");

        // Act
        let updated = client
            .execute(
                "UPDATE compat_match_simple_update_parent \
                 SET left_id = 10, right_id = 20 WHERE left_id = 1 AND right_id = 2",
                &[],
            )
            .await
            .expect("cascade the referenced composite key update");
        let rows = client
            .query(
                "SELECT id, left_id, right_id \
                 FROM compat_match_simple_update_child ORDER BY id",
                &[],
            )
            .await
            .expect("read children after the referenced key update");
        let metadata = client
            .query_one(
                "SELECT confmatchtype::TEXT, confupdtype::TEXT \
                 FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_match_simple_update_fk'",
                &[],
            )
            .await
            .expect("inspect MATCH and update-action catalog values");

        // Assert
        assert_eq!(updated, 1);
        assert_eq!(metadata.get::<_, String>(0), "s");
        assert_eq!(metadata.get::<_, String>(1), "c");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, Option<i32>>(1), Some(1));
        assert_eq!(rows[0].get::<_, Option<i32>>(2), None);
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, Option<i32>>(1), None);
        assert_eq!(rows[1].get::<_, Option<i32>>(2), Some(1));
        assert_eq!(rows[2].get::<_, i32>(0), 3);
        assert_eq!(rows[2].get::<_, Option<i32>>(1), Some(10));
        assert_eq!(rows[2].get::<_, Option<i32>>(2), Some(20));

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_set_only_listed_foreign_key_columns_to_default() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("foreign_key_set_default_column_subset").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_set_default_subset_parent (\
                   left_id INT, right_id INT, PRIMARY KEY (left_id, right_id))",
                "CREATE TABLE compat_set_default_subset_child (\
                   id INT PRIMARY KEY, left_id INT DEFAULT 3, right_id INT, \
                   CONSTRAINT compat_set_default_subset_fk \
                   FOREIGN KEY (left_id, right_id) \
                   REFERENCES compat_set_default_subset_parent (left_id, right_id) \
                   ON DELETE SET DEFAULT (left_id))",
                "INSERT INTO compat_set_default_subset_parent VALUES (1, 2), (3, 2)",
                "INSERT INTO compat_set_default_subset_child VALUES (10, 1, 2)",
            ],
        )
        .await
        .expect("create the composite foreign key with a one-column default action");

        // Act
        let deleted = client
            .execute(
                "DELETE FROM compat_set_default_subset_parent WHERE left_id = 1",
                &[],
            )
            .await
            .expect("delete the referenced parent row");
        let rows = client
            .query(
                "SELECT id, left_id, right_id FROM compat_set_default_subset_child",
                &[],
            )
            .await
            .expect("read the child after the subset DEFAULT action");

        // Assert
        assert_eq!(deleted, 1);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 10);
        assert_eq!(rows[0].get::<_, i32>(1), 3);
        assert_eq!(rows[0].get::<_, i32>(2), 2);

        drop(client);
        server.shutdown(connection).await;
    });
}
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
                "ALTER TABLE compat_constraint_index_dependency \
                   ADD CONSTRAINT compat_constraint_index_dependency_key \
                   UNIQUE USING INDEX compat_constraint_index_dependency_index",
            ],
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
                        index_class.relname \
                 FROM pg_catalog.pg_constraint AS constraint_row \
                 JOIN pg_catalog.pg_index AS index_row \
                   ON index_row.indexrelid = constraint_row.conindid \
                 JOIN pg_catalog.pg_class AS index_class \
                   ON index_class.oid = index_row.indexrelid \
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
