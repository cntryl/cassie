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
fn should_check_deferred_unique_conflict_when_constraint_becomes_immediate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("deferred_unique_constraint_check").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_deferred_unique_check (value INT, \
                   CONSTRAINT compat_deferred_unique_check_key UNIQUE (value) \
                   DEFERRABLE INITIALLY DEFERRED)",
            )
            .await
            .expect("create a named initially deferred unique constraint");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin a transaction for the deferred unique constraint");
        assert_eq!(
            client
                .execute("INSERT INTO compat_deferred_unique_check VALUES (7)", &[])
                .await
                .expect("insert the first deferred unique value"),
            1
        );
        assert_eq!(
            client
                .execute("INSERT INTO compat_deferred_unique_check VALUES (7)", &[])
                .await
                .expect("defer the duplicate unique check until the constraint changes mode"),
            1
        );

        // Act
        let immediate = client
            .batch_execute("SET CONSTRAINTS compat_deferred_unique_check_key IMMEDIATE")
            .await
            .expect_err("changing the constraint to IMMEDIATE must check queued duplicates");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the transaction after the failed constraint check");
        let row_count: i64 = client
            .query_one("SELECT count(*) FROM compat_deferred_unique_check", &[])
            .await
            .expect("read rows after rollback")
            .get(0);
        let metadata = client
            .query_one(
                "SELECT condeferrable, condeferred \
                 FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_deferred_unique_check_key'",
                &[],
            )
            .await
            .expect("read the retained constraint metadata");

        // Assert
        assert_eq!(db_error(&immediate).code().code(), "23505");
        assert_eq!(row_count, 0, "rollback must remove both duplicate rows");
        assert!(metadata.get::<_, bool>(0));
        assert!(metadata.get::<_, bool>(1));

        drop(client);
        server.shutdown(connection).await;
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

#[test]
fn should_honor_explicit_not_deferrable_constraint_clauses() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("explicit_not_deferrable_constraint").await;
        let (client, connection) = server.connect().await;
        let created = client
            .batch_execute(
                "CREATE TABLE compat_explicit_not_deferrable (value INT, \
                   CONSTRAINT compat_explicit_not_deferrable_key UNIQUE (value) \
                   NOT DEFERRABLE INITIALLY IMMEDIATE)",
            )
            .await;

        // Act
        let (metadata, duplicate, invalid_mode, invalid_relation_count) = if created.is_ok() {
            let metadata = client
                .query_one(
                    "SELECT condeferrable, condeferred, convalidated \
                     FROM pg_catalog.pg_constraint \
                     WHERE conname = 'compat_explicit_not_deferrable_key'",
                    &[],
                )
                .await;
            let first = client
                .execute("INSERT INTO compat_explicit_not_deferrable VALUES (1)", &[])
                .await;
            let duplicate = if first.is_ok() {
                Some(
                    client
                        .execute("INSERT INTO compat_explicit_not_deferrable VALUES (1)", &[])
                        .await,
                )
            } else {
                None
            };
            let invalid_mode = client
                .batch_execute(
                    "CREATE TABLE compat_invalid_not_deferrable (value INT, \
                     CONSTRAINT compat_invalid_not_deferrable_key UNIQUE (value) \
                     NOT DEFERRABLE INITIALLY DEFERRED)",
                )
                .await;
            let invalid_relation_count = client
                .query_one(
                    "SELECT count(*) FROM pg_catalog.pg_class \
                     WHERE relname = 'compat_invalid_not_deferrable'",
                    &[],
                )
                .await;
            (
                Some(metadata),
                duplicate,
                Some(invalid_mode),
                Some(invalid_relation_count),
            )
        } else {
            (None, None, None, None)
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(
            created.is_ok(),
            "explicit NOT DEFERRABLE INITIALLY IMMEDIATE should be accepted: {created:?}"
        );
        let metadata = metadata
            .expect("inspect the created NOT DEFERRABLE constraint")
            .expect("read constraint metadata");
        assert!(!metadata.get::<_, bool>(0));
        assert!(!metadata.get::<_, bool>(1));
        assert!(metadata.get::<_, bool>(2));
        let duplicate = duplicate
            .expect("attempt the duplicate after inserting the first key")
            .expect_err("NOT DEFERRABLE must reject a duplicate immediately");
        assert_eq!(db_error(&duplicate).code().code(), "23505");
        let invalid_mode = invalid_mode
            .expect("attempt the contradictory initially deferred declaration")
            .expect_err("NOT DEFERRABLE cannot be INITIALLY DEFERRED");
        assert_eq!(db_error(&invalid_mode).code().code(), "42601");
        assert_eq!(
            invalid_relation_count
                .expect("inspect rejected constraint relation publication")
                .expect("read relation count")
                .get::<_, i64>(0),
            0,
            "invalid constraint DDL must not publish its table"
        );
    });
}

#[test]
fn should_enforce_temporal_unique_without_overlaps_by_equality_prefix() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("temporal_unique_without_overlaps").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_temporal_unique_without_overlaps (\
                   grouping_key INT4RANGE NOT NULL, during TSTZRANGE NOT NULL, \
                   CONSTRAINT compat_temporal_unique_period_key \
                   UNIQUE (grouping_key, during WITHOUT OVERLAPS))",
                "INSERT INTO compat_temporal_unique_without_overlaps VALUES \
                   ('[1,2)', tstzrange('2026-01-01 00:00+00', '2026-01-05 00:00+00', '[)')), \
                   ('[1,2)', tstzrange('2026-01-05 00:00+00', '2026-01-10 00:00+00', '[)')), \
                   ('[2,3)', tstzrange('2026-01-02 00:00+00', '2026-01-04 00:00+00', '[)'))",
            ],
        )
        .await
        .expect("accept adjacent periods and overlapping periods under another key");

        // Act
        let overlap = client
            .execute(
                "INSERT INTO compat_temporal_unique_without_overlaps VALUES \
                 ('[1,2)', tstzrange('2026-01-04 00:00+00', '2026-01-06 00:00+00', '[)'))",
                &[],
            )
            .await
            .expect_err("a period must not overlap another period for the same key");
        let rows = client
            .query(
                "SELECT grouping_key::TEXT, during::TEXT \
                 FROM compat_temporal_unique_without_overlaps ORDER BY grouping_key, during",
                &[],
            )
            .await
            .expect("read rows after the rejected period overlap");

        // Assert
        assert_eq!(db_error(&overlap).code().code(), "23P01");
        assert_eq!(rows.len(), 3, "the rejected period must not be published");
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_cover_temporal_foreign_keys_across_adjacent_parent_periods() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("temporal_period_foreign_key").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_temporal_period_parent (\
                   grouping_key INT4RANGE NOT NULL, during TSTZRANGE NOT NULL, \
                   CONSTRAINT compat_temporal_period_parent_key \
                   UNIQUE (grouping_key, during WITHOUT OVERLAPS))",
                "CREATE TABLE compat_temporal_period_child (\
                   grouping_key INT4RANGE NOT NULL, during TSTZRANGE NOT NULL, \
                   CONSTRAINT compat_temporal_period_child_fk \
                   FOREIGN KEY (grouping_key, PERIOD during) \
                   REFERENCES compat_temporal_period_parent (grouping_key, PERIOD during))",
                "INSERT INTO compat_temporal_period_parent VALUES \
                   ('[1,2)', tstzrange('2026-01-01 00:00+00', '2026-01-05 00:00+00', '[)')), \
                   ('[1,2)', tstzrange('2026-01-05 00:00+00', '2026-01-10 00:00+00', '[)'))",
                "INSERT INTO compat_temporal_period_child VALUES \
                   ('[1,2)', tstzrange('2026-01-02 00:00+00', '2026-01-09 00:00+00', '[)'))",
            ],
        )
        .await
        .expect("a child period may be covered by adjacent parent periods");

        // Act
        let uncovered = client
            .execute(
                "INSERT INTO compat_temporal_period_child VALUES \
                 ('[1,2)', tstzrange('2025-12-31 00:00+00', '2026-01-02 00:00+00', '[)'))",
                &[],
            )
            .await
            .expect_err("a child period extending outside parent coverage must fail");
        let rows = client
            .query(
                "SELECT grouping_key::TEXT, during::TEXT \
                 FROM compat_temporal_period_child ORDER BY during",
                &[],
            )
            .await
            .expect("read the child period accepted before the uncovered insert");

        // Assert
        assert_eq!(db_error(&uncovered).code().code(), "23503");
        assert_eq!(
            rows.len(),
            1,
            "the uncovered child period must not be stored"
        );
        drop(client);
        server.shutdown(connection).await;
    });
}
