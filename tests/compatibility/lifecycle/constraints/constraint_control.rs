use super::*;

#[test]
fn should_enforce_table_unique_nulls_not_distinct_constraint() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("table_unique_nulls_not_distinct").await;
        let (client, connection) = server.connect().await;

        // Act
        let created = client
            .batch_execute(
                "CREATE TABLE compat_unique_nulls_not_distinct (value INT, \
                   CONSTRAINT compat_unique_nulls_not_distinct_key \
                   UNIQUE NULLS NOT DISTINCT (value))",
            )
            .await;
        let index_metadata = if created.is_ok() {
            Some(
                client
                    .query_one(
                        "SELECT constraint_row.conindid = index_row.indexrelid, \
                                index_row.indnullsnotdistinct \
                         FROM pg_catalog.pg_constraint AS constraint_row \
                         JOIN pg_catalog.pg_index AS index_row \
                           ON index_row.indexrelid = constraint_row.conindid \
                         WHERE constraint_row.conname = \
                           'compat_unique_nulls_not_distinct_key'",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };
        let (first_null, duplicate_null, rows) = if created.is_ok() {
            let first_null = client
                .execute(
                    "INSERT INTO compat_unique_nulls_not_distinct VALUES (NULL)",
                    &[],
                )
                .await;
            let duplicate_null = if first_null.is_ok() {
                Some(
                    client
                        .execute(
                            "INSERT INTO compat_unique_nulls_not_distinct VALUES (NULL)",
                            &[],
                        )
                        .await,
                )
            } else {
                None
            };
            let rows = if duplicate_null.as_ref().is_some_and(Result::is_err) {
                client
                    .query("SELECT value FROM compat_unique_nulls_not_distinct", &[])
                    .await
                    .ok()
            } else {
                None
            };
            (Some(first_null), duplicate_null, rows)
        } else {
            (None, None, None)
        };
        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(
            created.is_ok(),
            "table UNIQUE NULLS NOT DISTINCT should be accepted: {created:?}"
        );
        assert_eq!(
            first_null
                .expect("insert the first NULL key")
                .expect("the first NULL key should be accepted"),
            1
        );
        let index_metadata = index_metadata
            .expect("inspect the backing index metadata")
            .expect("read the unique constraint's backing index");
        assert!(index_metadata.get::<_, bool>(0));
        assert!(index_metadata.get::<_, bool>(1));
        let duplicate_null = duplicate_null
            .expect("attempt a duplicate NULL key")
            .expect_err("a table unique constraint must treat NULL keys as equal");
        assert_eq!(db_error(&duplicate_null).code().code(), "23505");
        assert_eq!(rows.expect("read the remaining row").len(), 1);
    });
}

#[test]
fn should_enforce_composite_unique_nulls_not_distinct_constraint() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("composite_unique_nulls_not_distinct").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_composite_unique_nulls_not_distinct \
                   (tenant_id INT, key_value INT, \
                    CONSTRAINT compat_composite_unique_nulls_not_distinct_key \
                    UNIQUE NULLS NOT DISTINCT (tenant_id, key_value))",
            )
            .await
            .expect("create a composite unique constraint with NND NULL policy");

        // Act
        client
            .execute(
                "INSERT INTO compat_composite_unique_nulls_not_distinct VALUES (1, NULL)",
                &[],
            )
            .await
            .expect("insert the first composite NULL key");
        client
            .execute(
                "INSERT INTO compat_composite_unique_nulls_not_distinct VALUES (2, NULL)",
                &[],
            )
            .await
            .expect("a different leading key may also have a NULL value");
        let duplicate_null = client
            .execute(
                "INSERT INTO compat_composite_unique_nulls_not_distinct VALUES (1, NULL)",
                &[],
            )
            .await
            .expect_err("a repeated composite key containing NULL must conflict");
        let metadata = client
            .query_one(
                "SELECT indnullsnotdistinct FROM pg_catalog.pg_index \
                 WHERE indexrelid = 'compat_composite_unique_nulls_not_distinct_key'",
                &[],
            )
            .await
            .expect("inspect the composite constraint backing index");

        // Assert
        assert_eq!(db_error(&duplicate_null).code().code(), "23505");
        assert!(metadata.get::<_, bool>(0));
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_drop_unique_constraint_backing_index_with_constraint() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("unique_constraint_index_lifecycle").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_unique_constraint_index (value INT, \
                   CONSTRAINT compat_unique_constraint_index_key UNIQUE (value))",
            )
            .await
            .expect("create a table with a unique constraint");

        // Act
        let blocked_index_drop = client
            .batch_execute("DROP INDEX compat_unique_constraint_index_key")
            .await
            .expect_err("the unique constraint must own its backing index");
        client
            .batch_execute(
                "ALTER TABLE compat_unique_constraint_index \
                 DROP CONSTRAINT compat_unique_constraint_index_key",
            )
            .await
            .expect("drop the constraint and its backing index");
        let remaining_indexes = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_class \
                 WHERE relname = 'compat_unique_constraint_index_key'",
                &[],
            )
            .await
            .expect("inspect index cleanup");

        // Assert
        assert_eq!(db_error(&blocked_index_drop).code().code(), "2BP01");
        assert_eq!(remaining_indexes.get::<_, i64>(0), 0);
        drop(client);
        server.shutdown(connection).await;
    });
}
