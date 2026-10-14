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
        let duplicate_null = duplicate_null
            .expect("attempt a duplicate NULL key")
            .expect_err("a table unique constraint must treat NULL keys as equal");
        assert_eq!(db_error(&duplicate_null).code().code(), "23505");
        assert_eq!(rows.expect("read the remaining row").len(), 1);
    });
}

#[test]
fn should_preserve_named_unique_constraint_deferrability_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("unique_constraint_deferrability").await;
        let (client, connection) = server.connect().await;
        let created = client
            .batch_execute(
                "CREATE TABLE compat_deferrable_unique (value INT, \
                   CONSTRAINT compat_deferrable_unique_key UNIQUE (value) \
                   DEFERRABLE INITIALLY IMMEDIATE)",
            )
            .await;

        // Act
        let metadata = if created.is_ok() {
            Some(
                client
                    .query_one(
                        "SELECT condeferrable, condeferred \
                         FROM pg_catalog.pg_constraint \
                         WHERE conname = 'compat_deferrable_unique_key'",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(
            created.is_ok(),
            "a named deferrable unique constraint should be accepted: {created:?}"
        );
        let metadata = metadata
            .expect("inspect the created deferrable unique constraint")
            .expect("read deferrability metadata");
        assert!(metadata.get::<_, bool>(0));
        assert!(!metadata.get::<_, bool>(1));
    });
}

#[test]
fn should_preserve_named_foreign_key_deferrability_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("foreign_key_deferrability").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_constraint_parent (id INT PRIMARY KEY)")
            .await
            .expect("create the parent key");
        let child_created = client
            .batch_execute(
                "CREATE TABLE compat_constraint_child (parent_id INT, \
                   CONSTRAINT compat_constraint_child_parent_fk \
                   FOREIGN KEY (parent_id) REFERENCES compat_constraint_parent (id) \
                   DEFERRABLE INITIALLY IMMEDIATE)",
            )
            .await;

        // Act
        let metadata = if child_created.is_ok() {
            Some(
                client
                    .query_one(
                        "SELECT condeferrable, condeferred \
                         FROM pg_catalog.pg_constraint \
                         WHERE conname = 'compat_constraint_child_parent_fk'",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(
            child_created.is_ok(),
            "a deferrable foreign key should be accepted: {child_created:?}"
        );
        let metadata = metadata
            .expect("inspect the created deferrable foreign key")
            .expect("read deferrability metadata");
        assert!(metadata.get::<_, bool>(0));
        assert!(!metadata.get::<_, bool>(1));
    });
}
