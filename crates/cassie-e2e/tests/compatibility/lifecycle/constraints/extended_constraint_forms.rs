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
                     UNIQUE (id) INCLUDE (payload) \
                     WITH (fillfactor=80, deduplicate_items=off)\
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
                   ON index_class.oid = index_row.indexrelid_oid \
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
        assert_eq!(
            metadata.get::<_, String>(3),
            "{deduplicate_items=false,fillfactor=80}"
        );
        assert_eq!(db_error(&duplicate).code().code(), "23505");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_unique_include_field_that_repeats_key() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("invalid_unique_include_fields").await;
        let (client, connection) = server.connect().await;

        // Act
        let key_overlap = client
            .batch_execute(
                "CREATE TABLE compat_unique_include_key_overlap \
                   (id INT, payload TEXT, UNIQUE (id) INCLUDE (id))",
            )
            .await
            .expect_err("reject an INCLUDE column that repeats a unique key");
        // Assert
        assert_eq!(db_error(&key_overlap).code().code(), "42601");
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_duplicate_unique_include_fields() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("duplicate_unique_include_fields").await;
        let (client, connection) = server.connect().await;

        // Act
        let duplicate_include = client
            .batch_execute(
                "CREATE TABLE compat_unique_include_duplicate \
                   (id INT, payload TEXT, \
                    UNIQUE (id) INCLUDE (payload, payload))",
            )
            .await
            .expect_err("reject duplicate INCLUDE columns");

        // Assert
        assert_eq!(db_error(&duplicate_include).code().code(), "42601");
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_normalize_include_field_spelling_for_altered_unique_constraints() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("alter_unique_include_spelling").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_alter_include_rows \
                   (id INT, \"CamelPayload\" TEXT)",
            )
            .await
            .expect("create the table that will receive the unique constraint");

        // Act
        client
            .batch_execute(
                "ALTER TABLE compat_alter_include_rows \
                 ADD CONSTRAINT compat_alter_include_key \
                 UNIQUE (id) INCLUDE (\"CamelPayload\") WITH (fillfactor=77)",
            )
            .await
            .expect("add a unique constraint with a quoted INCLUDE field");
        let metadata = client
            .query_one(
                "SELECT index_row.indnatts::INT, index_class.reloptions::TEXT \
                 FROM pg_catalog.pg_index AS index_row \
                 JOIN pg_catalog.pg_class AS index_class \
                   ON index_class.oid = index_row.indexrelid_oid \
                 JOIN pg_catalog.pg_constraint AS constraint_row \
                   ON constraint_row.conindid = index_row.indexrelid \
                 WHERE constraint_row.conname = 'compat_alter_include_key'",
                &[],
            )
            .await
            .expect("inspect the backing index include metadata");

        // Assert
        assert_eq!(metadata.get::<_, i32>(0), 2);
        assert_eq!(metadata.get::<_, String>(1), "{fillfactor=77}");
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_unknown_include_field_on_altered_unique_constraint() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("alter_unique_include_unknown").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_alter_include_unknown (id INT)")
            .await
            .expect("create the table that will reject the constraint");

        // Act
        let error = client
            .batch_execute(
                "ALTER TABLE compat_alter_include_unknown \
                 ADD CONSTRAINT compat_alter_include_unknown_key \
                 UNIQUE (id) INCLUDE (missing)",
            )
            .await
            .expect_err("reject an INCLUDE field that is absent from the table");

        // Assert
        assert!(db_error(&error)
            .message()
            .contains("INCLUDE field 'missing'"));
        let published = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_alter_include_unknown_key'",
                &[],
            )
            .await
            .expect("check that invalid constraint metadata was not published")
            .get::<_, i64>(0);
        assert_eq!(published, 0);
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
            .batch_execute("CREATE TABLE compat_attach_constraint_rows (id INT, payload TEXT)")
            .await
            .expect("create the table that will own the constraint");
        client
            .batch_execute(
                "CREATE UNIQUE INDEX compat_attach_constraint_key \
                 ON compat_attach_constraint_rows (id) INCLUDE (payload) \
                 WITH (fillfactor=73, deduplicate_items=off)",
            )
            .await
            .expect("create the unique index to attach");

        // Act
        client
            .batch_execute(
                "ALTER TABLE compat_attach_constraint_rows \
                 ADD CONSTRAINT compat_attach_constraint_key \
                 UNIQUE USING INDEX compat_attach_constraint_key",
            )
            .await
            .expect("attach the existing unique index to a named constraint");
        let metadata = client
            .query_one(
                "SELECT constraint_row.conindid = index_row.indexrelid, \
                        index_class.reloptions::TEXT \
                 FROM pg_catalog.pg_constraint AS constraint_row \
                 JOIN pg_catalog.pg_index AS index_row \
                   ON index_row.indexrelid = constraint_row.conindid \
                 JOIN pg_catalog.pg_class AS index_class \
                   ON index_class.oid = index_row.indexrelid_oid \
                 WHERE constraint_row.conname = 'compat_attach_constraint_key' \
                   AND index_class.relname = 'compat_attach_constraint_key'",
                &[],
            )
            .await
            .expect("read the constraint-to-index catalog dependency");
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
        assert!(metadata.get::<_, bool>(0));
        assert_eq!(
            metadata.get::<_, String>(1),
            "{deduplicate_items=off,fillfactor=73}"
        );
        assert_eq!(db_error(&duplicate).code().code(), "23505");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_use_index_name_for_an_unnamed_unique_constraint() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("unnamed_unique_index_constraint").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_unnamed_attach_rows (id INT)")
            .await
            .expect("create the table that will own the constraint");
        client
            .batch_execute(
                "CREATE UNIQUE INDEX compat_unnamed_attach_key \
                 ON compat_unnamed_attach_rows (id)",
            )
            .await
            .expect("create the unique index to attach");

        // Act
        client
            .batch_execute(
                "ALTER TABLE compat_unnamed_attach_rows \
                 ADD UNIQUE USING INDEX compat_unnamed_attach_key",
            )
            .await
            .expect("attach the index without an explicit constraint name");
        let catalog_rows = client
            .query(
                "SELECT constraint_row.conname, constraint_row.conindid, \
                        index_row.indexrelid \
                 FROM pg_catalog.pg_constraint AS constraint_row \
                 LEFT JOIN pg_catalog.pg_index AS index_row \
                   ON index_row.indexrelid = constraint_row.conindid \
                 WHERE constraint_row.conrelid = 'compat_unnamed_attach_rows' \
                   AND constraint_row.contype = 'u'",
                &[],
            )
            .await
            .expect("inspect the implicit constraint and backing-index names");
        let catalog_values = catalog_rows
            .iter()
            .map(|row| {
                (
                    row.get::<_, String>(0),
                    row.get::<_, Option<String>>(1),
                    row.get::<_, Option<String>>(2),
                )
            })
            .collect::<Vec<_>>();
        let attached = catalog_values.iter().any(|(name, backing, index)| {
            name == "compat_unnamed_attach_key"
                && backing.as_deref() == Some("compat_unnamed_attach_key")
                && index.as_deref() == Some("compat_unnamed_attach_key")
        });
        assert!(attached, "catalog values: {catalog_values:?}");
        client
            .batch_execute(
                "ALTER TABLE compat_unnamed_attach_rows \
                 DROP CONSTRAINT compat_unnamed_attach_key",
            )
            .await
            .expect("drop the implicit constraint and its index");
        let remaining: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_class \
                 WHERE relname = 'compat_unnamed_attach_key'",
                &[],
            )
            .await
            .expect("check that the owned index was removed")
            .get(0);

        // Assert
        assert_eq!(remaining, 0);
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_keep_composite_unique_enforcement_after_constraint_adoption() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("attach_composite_unique_index").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_attach_composite_rows \
                   (tenant_id INT, item_id INT, payload TEXT)",
            )
            .await
            .expect("create the table that will own the composite constraint");
        client
            .batch_execute(
                "CREATE UNIQUE INDEX compat_attach_composite_index \
                 ON compat_attach_composite_rows (tenant_id, item_id)",
            )
            .await
            .expect("create a unique composite index");
        client
            .execute(
                "INSERT INTO compat_attach_composite_rows VALUES (1, 2, 'existing')",
                &[],
            )
            .await
            .expect("insert the row whose index reservation will be adopted");
        client
            .batch_execute(
                "ALTER TABLE compat_attach_composite_rows \
                 ADD CONSTRAINT compat_attach_composite_key \
                 UNIQUE USING INDEX compat_attach_composite_index",
            )
            .await
            .expect("adopt and rename the existing composite unique index");

        // Act
        let duplicate = client
            .execute(
                "INSERT INTO compat_attach_composite_rows VALUES (1, 2, 'duplicate')",
                &[],
            )
            .await
            .expect_err("reject a duplicate key after index adoption");
        client
            .batch_execute(
                "ALTER TABLE compat_attach_composite_rows \
                 DROP CONSTRAINT compat_attach_composite_key",
            )
            .await
            .expect("drop the adopted constraint and its backing index");
        let after_drop = client
            .execute(
                "INSERT INTO compat_attach_composite_rows VALUES (1, 2, 'after drop')",
                &[],
            )
            .await
            .expect("allow the duplicate after dropping its constraint");

        // Assert
        assert_eq!(db_error(&duplicate).code().code(), "23505");
        assert_eq!(after_drop, 1);
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_nulls_not_distinct_when_adopting_unique_index() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("attach_nulls_not_distinct_index").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_attach_nulls_rows (id INT, key_value INT)")
            .await
            .expect("create the table that will own the unique constraint");
        client
            .batch_execute(
                "CREATE UNIQUE INDEX compat_attach_nulls_index \
                 ON compat_attach_nulls_rows (key_value) NULLS NOT DISTINCT",
            )
            .await
            .expect("create an index that treats NULL keys as equal");
        client
            .execute("INSERT INTO compat_attach_nulls_rows VALUES (1, NULL)", &[])
            .await
            .expect("insert the first NULL key before attachment");
        client
            .batch_execute(
                "ALTER TABLE compat_attach_nulls_rows \
                 ADD CONSTRAINT compat_attach_nulls_key \
                 UNIQUE USING INDEX compat_attach_nulls_index",
            )
            .await
            .expect("adopt and rename the NULLS NOT DISTINCT index");

        // Act
        let reported_policy: bool = client
            .query_one(
                "SELECT indnullsnotdistinct FROM pg_catalog.pg_index \
                 WHERE indexrelid = 'compat_attach_nulls_key'",
                &[],
            )
            .await
            .expect("read the adopted index NULL policy")
            .get(0);
        let duplicate_null = client
            .execute("INSERT INTO compat_attach_nulls_rows VALUES (2, NULL)", &[])
            .await
            .expect_err("reject another NULL key after attachment");

        // Assert
        assert!(reported_policy);
        assert_eq!(db_error(&duplicate_null).code().code(), "23505");
        drop(client);
        server.shutdown(connection).await;
    });
}
