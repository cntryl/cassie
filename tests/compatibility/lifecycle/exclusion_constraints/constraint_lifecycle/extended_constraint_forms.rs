use super::*;

#[test]
fn should_preserve_include_columns_on_a_unique_constraint_index() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("unique_constraint_include_columns").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_include_constraint_rows (\
                   id INT, payload TEXT, \
                   CONSTRAINT compat_include_constraint_key \
                     UNIQUE (id) INCLUDE (payload) WITH (fillfactor=80)\
                 )",
            )
            .await
            .expect("create a unique constraint with an included column and index option");
        client
            .execute(
                "INSERT INTO compat_include_constraint_rows VALUES (1, 'first')",
                &[],
            )
            .await
            .expect("insert the first key covered by the unique constraint");

        // Act
        let metadata = client
            .query_one(
                "SELECT index_row.indnatts::INT, index_row.indnkeyatts::INT, \
                        constraint_row.conindid = index_row.indexrelid, \
                        index_class.reloptions::TEXT \
                 FROM pg_catalog.pg_index AS index_row \
                 JOIN pg_catalog.pg_class AS index_class \
                   ON index_class.oid = index_row.indexrelid \
                 JOIN pg_catalog.pg_constraint AS constraint_row \
                   ON constraint_row.conindid = index_row.indexrelid \
                 WHERE constraint_row.conname = 'compat_include_constraint_key'",
                &[],
            )
            .await
            .expect("inspect the index backing the table-level unique constraint");
        let duplicate = client
            .execute(
                "INSERT INTO compat_include_constraint_rows VALUES (1, 'duplicate')",
                &[],
            )
            .await
            .expect_err("reject a duplicate key regardless of its included payload");

        // Assert
        assert_eq!(metadata.get::<_, i32>(0), 2);
        assert_eq!(metadata.get::<_, i32>(1), 1);
        assert!(metadata.get::<_, bool>(2));
        assert_eq!(metadata.get::<_, String>(3), "{fillfactor=80}");
        assert_eq!(db_error(&duplicate).code().code(), "23505");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_attach_an_existing_unique_index_as_a_table_constraint() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("attach_unique_index_constraint").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_attach_constraint_rows (id INT, payload TEXT); \
                 CREATE UNIQUE INDEX compat_attach_constraint_key \
                   ON compat_attach_constraint_rows (id) INCLUDE (payload)",
            )
            .await
            .expect("create a table and a matching unique index");

        // Act
        client
            .batch_execute(
                "ALTER TABLE compat_attach_constraint_rows \
                 ADD CONSTRAINT compat_attach_constraint_key \
                 UNIQUE USING INDEX compat_attach_constraint_key",
            )
            .await
            .expect("attach the existing unique index to a named constraint");
        let attached: bool = client
            .query_one(
                "SELECT constraint_row.conindid = index_row.indexrelid \
                 FROM pg_catalog.pg_constraint AS constraint_row \
                 JOIN pg_catalog.pg_index AS index_row \
                   ON index_row.indexrelid = constraint_row.conindid \
                 JOIN pg_catalog.pg_class AS index_class \
                   ON index_class.oid = index_row.indexrelid \
                 WHERE constraint_row.conname = 'compat_attach_constraint_key' \
                   AND index_class.relname = 'compat_attach_constraint_key'",
                &[],
            )
            .await
            .expect("read the constraint-to-index catalog dependency")
            .get(0);
        client
            .execute(
                "INSERT INTO compat_attach_constraint_rows VALUES (1, 'first')",
                &[],
            )
            .await
            .expect("insert a key after attaching its backing index");
        let duplicate = client
            .execute(
                "INSERT INTO compat_attach_constraint_rows VALUES (1, 'duplicate')",
                &[],
            )
            .await
            .expect_err("enforce the attached constraint for later inserts");

        // Assert
        assert!(attached);
        assert_eq!(db_error(&duplicate).code().code(), "23505");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_defer_a_foreign_key_after_altering_its_deferrability() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("alter_foreign_key_deferrability").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_alter_fk_parent (id INT PRIMARY KEY)",
                "CREATE TABLE compat_alter_fk_child (parent_id INT)",
                "ALTER TABLE compat_alter_fk_child \
                   ADD CONSTRAINT compat_alter_fk_name \
                   FOREIGN KEY (parent_id) REFERENCES compat_alter_fk_parent(id)",
                "ALTER TABLE compat_alter_fk_child \
                   ALTER CONSTRAINT compat_alter_fk_name \
                   DEFERRABLE INITIALLY DEFERRED",
            ],
        )
        .await
        .expect("alter a foreign key from immediate to initially deferred");

        // Act
        client
            .batch_execute(
                "BEGIN; \
                 INSERT INTO compat_alter_fk_child VALUES (18); \
                 INSERT INTO compat_alter_fk_parent VALUES (18); \
                 COMMIT",
            )
            .await
            .expect("check the foreign key at commit after its parent row is added");
        let metadata = client
            .query_one(
                "SELECT condeferrable, condeferred \
                 FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_alter_fk_name'",
                &[],
            )
            .await
            .expect("read the altered foreign-key deferrability flags");
        let matching_rows: i64 = client
            .query_one(
                "SELECT count(*) FROM compat_alter_fk_child AS child \
                 JOIN compat_alter_fk_parent AS parent ON parent.id = child.parent_id",
                &[],
            )
            .await
            .expect("verify the committed child has a matching parent")
            .try_get(0)
            .expect("matching row count");

        // Assert
        assert!(metadata.get::<_, bool>(0));
        assert!(metadata.get::<_, bool>(1));
        assert_eq!(matching_rows, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
