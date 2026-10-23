struct LikeAllObservations {
    copied_default: tokio_postgres::Row,
    duplicate_all_label: tokio_postgres::Error,
    excluded_rows: u64,
    all_label_metadata: tokio_postgres::Row,
    excluded_label_default: tokio_postgres::Row,
    all_identity: tokio_postgres::Row,
    all_checks: i64,
    excluded_checks: i64,
    all_indexes: i64,
    excluded_indexes: i64,
}

async fn read_like_all_catalog(
    client: &tokio_postgres::Client,
) -> (
    tokio_postgres::Row,
    tokio_postgres::Row,
    tokio_postgres::Row,
    i64,
    i64,
    i64,
    i64,
) {
    let included_label = client
        .query_one(
            "SELECT pg_get_expr(d.adbin, d.adrelid), \
                    col_description(c.oid, a.attnum), \
                    a.attstorage::TEXT, a.attcompression::TEXT \
             FROM pg_class c \
             JOIN pg_attribute a ON a.attrelid = c.oid \
             LEFT JOIN pg_attrdef d ON d.adrelid = c.oid AND d.adnum = a.attnum \
             WHERE c.relname = 'compat_like_options_all' AND a.attname = 'label'",
            &[],
        )
        .await
        .expect("inspect included column metadata");
    let excluded_label_default = client
        .query_one(
            "SELECT pg_get_expr(d.adbin, d.adrelid) \
             FROM pg_class c \
             JOIN pg_attribute a ON a.attrelid = c.oid \
             LEFT JOIN pg_attrdef d ON d.adrelid = c.oid AND d.adnum = a.attnum \
             WHERE c.relname = 'compat_like_options_excluded' AND a.attname = 'label'",
            &[],
        )
        .await
        .expect("inspect excluded default metadata");
    let all_identity = client
        .query_one(
            "SELECT attidentity::TEXT FROM pg_attribute \
             WHERE attrelid = 'compat_like_options_all'::regclass AND attname = 'id'",
            &[],
        )
        .await
        .expect("inspect included identity metadata");
    let all_checks = count_like_constraints(client, "compat_like_options_all").await;
    let excluded_checks = count_like_constraints(client, "compat_like_options_excluded").await;
    let all_indexes = count_like_indexes(client, "compat_like_options_all").await;
    let excluded_indexes = count_like_indexes(client, "compat_like_options_excluded").await;
    (
        included_label,
        excluded_label_default,
        all_identity,
        all_checks,
        excluded_checks,
        all_indexes,
        excluded_indexes,
    )
}

async fn count_like_constraints(client: &tokio_postgres::Client, table: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*) FROM pg_constraint \
             WHERE conrelid = $1::regclass AND contype = 'c'",
            &[&table],
        )
        .await
        .expect("count copied CHECK constraints")
        .get(0)
}

async fn count_like_indexes(client: &tokio_postgres::Client, table: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*) FROM pg_index WHERE indrelid = $1::regclass",
            &[&table],
        )
        .await
        .expect("count copied indexes")
        .get(0)
}

async fn observe_like_all_copy(client: &tokio_postgres::Client) -> LikeAllObservations {
    let copied_default = client
        .query_one(
            "INSERT INTO compat_like_options_all (id) VALUES (1) RETURNING label",
            &[],
        )
        .await
        .expect("INCLUDING ALL should copy the source default");
    let duplicate_all_label = client
        .execute(
            "INSERT INTO compat_like_options_all (id, label) VALUES (2, 'template')",
            &[],
        )
        .await
        .expect_err("INCLUDING ALL should copy the unique index");
    let excluded_rows = client
        .execute(
            "INSERT INTO compat_like_options_excluded (id, label) \
             VALUES (1, 'blocked'), (1, 'blocked')",
            &[],
        )
        .await
        .expect("EXCLUDING options should omit defaults, checks, and indexes");
    let (
        all_label_metadata,
        excluded_label_default,
        all_identity,
        all_checks,
        excluded_checks,
        all_indexes,
        excluded_indexes,
    ) = read_like_all_catalog(client).await;
    LikeAllObservations {
        copied_default,
        duplicate_all_label,
        excluded_rows,
        all_label_metadata,
        excluded_label_default,
        all_identity,
        all_checks,
        excluded_checks,
        all_indexes,
        excluded_indexes,
    }
}

#[test]
fn should_apply_like_all_with_explicit_exclusions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_all_explicit_exclusions").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_like_options_source (\
                   id INT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,\
                   label TEXT NOT NULL DEFAULT 'template',\
                   CONSTRAINT compat_like_options_check CHECK (label <> 'blocked'),\
                   UNIQUE (label)\
                 )",
                "COMMENT ON COLUMN compat_like_options_source.label IS 'copied comment'",
                "ALTER TABLE compat_like_options_source ALTER COLUMN label SET STORAGE EXTERNAL",
                "ALTER TABLE compat_like_options_source ALTER COLUMN label SET COMPRESSION pglz",
                "CREATE TABLE compat_like_options_all (\
                   LIKE compat_like_options_source INCLUDING ALL EXCLUDING IDENTITY\
                 )",
                "CREATE TABLE compat_like_options_excluded (\
                   LIKE compat_like_options_source INCLUDING ALL EXCLUDING DEFAULTS\
                   EXCLUDING CONSTRAINTS EXCLUDING INDEXES EXCLUDING IDENTITY\
                 )",
            ],
        )
        .await
        .expect("create LIKE source and combined option copies");

        // Act
        let observations = observe_like_all_copy(&client).await;

        // Assert
        assert_eq!(observations.copied_default.get::<_, String>(0), "template");
        assert_eq!(
            db_error(&observations.duplicate_all_label).code().code(),
            "23505"
        );
        assert_eq!(observations.excluded_rows, 2);
        assert_eq!(
            observations.all_label_metadata.get::<_, String>(0),
            "'template'::text"
        );
        assert_eq!(
            observations.all_label_metadata.get::<_, String>(1),
            "copied comment"
        );
        assert_eq!(observations.all_label_metadata.get::<_, String>(2), "e");
        assert_eq!(observations.all_label_metadata.get::<_, String>(3), "p");
        assert_eq!(
            observations
                .excluded_label_default
                .get::<_, Option<String>>(0),
            None
        );
        assert_eq!(observations.all_identity.get::<_, String>(0), "");
        assert_eq!(observations.all_checks, 1);
        assert_eq!(observations.excluded_checks, 0);
        assert_eq!(observations.all_indexes, 2);
        assert_eq!(observations.excluded_indexes, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_table_like_before_creating_the_target() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_ast_execution_guard").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_like_guard_source (id INT)")
            .await
            .expect("create LIKE source table");

        // Act
        let error = client
            .batch_execute(
                "CREATE TABLE compat_like_guard_target \
                 (LIKE compat_like_guard_source INCLUDING ALL)",
            )
            .await
            .expect_err("LIKE copy execution is still pending");
        let error = db_error(&error);
        let sqlstate = error.code().code().to_owned();
        let message = error.message().to_owned();
        let target_exists: bool = client
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
                 WHERE table_name = 'compat_like_guard_target')",
                &[],
            )
            .await
            .expect("check that rejected LIKE did not create its target")
            .get(0);

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(sqlstate, "0A000");
        assert!(
            message.contains("CREATE TABLE LIKE option INCLUDING ALL is not supported"),
            "unexpected error message: {message}"
        );
        assert!(!target_exists);
    });
}

#[test]
fn should_reject_table_like_before_if_not_exists_short_circuit() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_ast_if_not_exists_guard").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_like_short_circuit_source (id INT)")
            .await
            .expect("create source table");
        client
            .batch_execute("CREATE TABLE compat_like_short_circuit_target (id INT)")
            .await
            .expect("create target table");
        client
            .batch_execute("INSERT INTO compat_like_short_circuit_target VALUES (7)")
            .await
            .expect("seed target row");

        // Act
        let error = client
            .batch_execute(
                "CREATE TABLE IF NOT EXISTS compat_like_short_circuit_target \
                 (LIKE compat_like_short_circuit_source INCLUDING ALL)",
            )
            .await
            .expect_err("LIKE guard must run before IF NOT EXISTS");
        let error = db_error(&error);
        let sqlstate = error.code().code().to_owned();
        let message = error.message().to_owned();
        let retained_value: i32 = client
            .query_one(
                "SELECT id FROM compat_like_short_circuit_target",
                &[],
            )
            .await
            .expect("existing target remains readable")
            .get(0);

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(sqlstate, "0A000");
        assert!(
            message.contains("CREATE TABLE LIKE option INCLUDING ALL is not supported"),
            "unexpected error message: {message}"
        );
        assert_eq!(retained_value, 7);
    });
}
#[test]
fn should_preserve_ctas_type_modifiers_without_materializing_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("ctas_type_modifiers_no_data").await;
        let (client, connection) = server.connect().await;

        // Act
        let created = client
            .batch_execute(
                "CREATE TABLE compat_ctas_metadata (amount, label) AS \
                 SELECT 12::NUMERIC(8, 3), 'short'::VARCHAR(7) \
                 WHERE FALSE WITH NO DATA",
            )
            .await;
        let metadata = if created.is_ok() {
            Some(
                client
                    .query(
                        "SELECT a.attname, a.atttypid::INT, a.atttypmod, a.attnotnull \
                         FROM pg_attribute a \
                         WHERE a.attrelid = 'compat_ctas_metadata' \
                           AND a.attnum > 0 \
                         ORDER BY a.attnum",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };
        let row_count = if created.is_ok() {
            Some(
                client
                    .query_one("SELECT count(*) FROM compat_ctas_metadata", &[])
                    .await,
            )
        } else {
            None
        };

        // Assert
        let create_sqlstate = created
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code());
        drop(client);
        server.shutdown(connection).await;

        assert!(
            created.is_ok(),
            "CTAS WITH NO DATA should preserve result type modifiers; SQLSTATE: {create_sqlstate:?}"
        );
        let metadata = metadata
            .expect("successful CTAS should expose catalog metadata")
            .expect("read CTAS catalog metadata");
        assert_eq!(metadata.len(), 2);
        assert_eq!(metadata[0].get::<_, String>(0), "amount");
        assert_eq!(metadata[0].get::<_, i32>(1), 1700);
        assert_eq!(metadata[0].get::<_, i32>(2), 524_295);
        assert!(!metadata[0].get::<_, bool>(3));
        assert_eq!(metadata[1].get::<_, String>(0), "label");
        assert_eq!(metadata[1].get::<_, i32>(1), 1043);
        assert_eq!(metadata[1].get::<_, i32>(2), 11);
        assert!(!metadata[1].get::<_, bool>(3));
        assert_eq!(
            row_count
                .expect("successful CTAS should remain queryable")
                .expect("count rows in CTAS table")
                .get::<_, i64>(0),
            0
        );
    });
}
#[test]
fn should_track_materialized_view_population_through_refresh_no_data() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_population_state").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_mv_population_source (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_mv_population_source VALUES (1, 'one'), (2, 'two')",
                "CREATE MATERIALIZED VIEW compat_mv_population (item_id, item_label) AS \
                 SELECT id, label FROM compat_mv_population_source WITH DATA",
            ],
        )
        .await
        .expect("create populated materialized view");

        // Act
        let initially_populated: bool = client
            .query_one(
                "SELECT ispopulated FROM pg_matviews \
                 WHERE matviewname = 'compat_mv_population'",
                &[],
            )
            .await
            .expect("read initial materialized-view state")
            .try_get(0)
            .expect("initial ispopulated value");
        client
            .execute(
                "INSERT INTO compat_mv_population_source VALUES (3, 'three')",
                &[],
            )
            .await
            .expect("add a source row before refreshing");
        client
            .batch_execute("REFRESH MATERIALIZED VIEW compat_mv_population WITH NO DATA")
            .await
            .expect("mark materialized view unpopulated");
        let unpopulated: bool = client
            .query_one(
                "SELECT ispopulated FROM pg_matviews \
                 WHERE matviewname = 'compat_mv_population'",
                &[],
            )
            .await
            .expect("read unpopulated materialized-view state")
            .try_get(0)
            .expect("unpopulated ispopulated value");
        let scan_error = client
            .query("SELECT item_id FROM compat_mv_population", &[])
            .await
            .expect_err("an unpopulated materialized view cannot be scanned");
        client
            .batch_execute("REFRESH MATERIALIZED VIEW compat_mv_population WITH DATA")
            .await
            .expect("repopulate materialized view");
        let repopulated: bool = client
            .query_one(
                "SELECT ispopulated FROM pg_matviews \
                 WHERE matviewname = 'compat_mv_population'",
                &[],
            )
            .await
            .expect("read repopulated materialized-view state")
            .try_get(0)
            .expect("repopulated ispopulated value");
        let rows = client
            .query(
                "SELECT item_id, item_label FROM compat_mv_population ORDER BY item_id",
                &[],
            )
            .await
            .expect("read refreshed materialized-view rows");

        // Assert
        assert!(initially_populated);
        assert!(!unpopulated);
        assert_eq!(db_error(&scan_error).code().code(), "55000");
        assert!(repopulated);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "one");
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, String>(1), "two");
        assert_eq!(rows[2].get::<_, i32>(0), 3);
        assert_eq!(rows[2].get::<_, String>(1), "three");

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_preserve_existing_materialized_view_when_created_if_not_exists() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_if_not_exists").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_mv_if_not_exists_source (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_mv_if_not_exists_source VALUES (1, 'original')",
                "CREATE MATERIALIZED VIEW IF NOT EXISTS compat_mv_if_not_exists \
                   (view_id, view_label) AS \
                   SELECT id, label FROM compat_mv_if_not_exists_source WITH DATA",
                "CREATE MATERIALIZED VIEW IF NOT EXISTS compat_mv_if_not_exists \
                   (view_id, view_label) AS SELECT 999, 'replacement' WITH DATA",
            ],
        )
        .await
        .expect("create materialized view and skip duplicate IF NOT EXISTS form");

        // Act
        let original_rows = client
            .query(
                "SELECT view_id, view_label FROM compat_mv_if_not_exists ORDER BY view_id",
                &[],
            )
            .await
            .expect("read existing materialized-view contents");
        let catalog_entry = client
            .query_one(
                "SELECT matviewname, ispopulated FROM pg_matviews \
                 WHERE matviewname = 'compat_mv_if_not_exists'",
                &[],
            )
            .await
            .expect("inspect materialized-view catalog row");

        // Assert
        assert_eq!(original_rows.len(), 1);
        assert_eq!(original_rows[0].get::<_, i32>(0), 1);
        assert_eq!(original_rows[0].get::<_, String>(1), "original");
        assert_eq!(catalog_entry.get::<_, String>(0), "compat_mv_if_not_exists");
        assert!(catalog_entry.get::<_, bool>(1));

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_reject_materialized_view_rename_until_lifecycle_support_is_implemented() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_rename_catalog").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE MATERIALIZED VIEW compat_mv_rename AS SELECT 1 AS view_id")
            .await
            .expect("create materialized view before testing rename rejection");

        // Act
        let result = client
            .batch_execute(
                "ALTER MATERIALIZED VIEW compat_mv_rename RENAME TO compat_mv_renamed",
            )
            .await;
        let error = result.expect_err("materialized-view rename remains pending under #839");
        let catalog = client
            .query_one(
                "SELECT count(*) FILTER (WHERE matviewname = 'compat_mv_rename'), \
                        count(*) FILTER (WHERE matviewname = 'compat_mv_renamed') \
                 FROM pg_matviews",
                &[],
            )
            .await
            .expect("inspect names after the rejected rename");

        // Assert
        assert_eq!(db_error(&error).code().code(), "0A000");
        assert_eq!(catalog.get::<_, i64>(0), 1);
        assert_eq!(catalog.get::<_, i64>(1), 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_remove_dropped_materialized_view_from_pg_matviews() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_drop_catalog").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE MATERIALIZED VIEW compat_mv_drop AS SELECT 1 AS view_id")
            .await
            .expect("create materialized view before dropping");

        // Act
        client
            .batch_execute("DROP MATERIALIZED VIEW compat_mv_drop")
            .await
            .expect("drop materialized view");
        let catalog_rows: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_matviews WHERE matviewname = 'compat_mv_drop'",
                &[],
            )
            .await
            .expect("inspect catalog after materialized-view drop")
            .try_get(0)
            .expect("remaining materialized-view rows");

        // Assert
        assert_eq!(catalog_rows, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_require_a_unique_index_for_concurrent_materialized_view_refresh() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_concurrent_refresh").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_mv_concurrent_source (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_mv_concurrent_source VALUES (1, 'one'), (2, 'two')",
                "CREATE MATERIALIZED VIEW compat_mv_concurrent (item_id, item_label) AS \
                   SELECT id, label FROM compat_mv_concurrent_source WITH DATA",
            ],
        )
        .await
        .expect("create materialized view for concurrent refresh");

        // Act
        let missing_unique_index = client
            .batch_execute("REFRESH MATERIALIZED VIEW CONCURRENTLY compat_mv_concurrent")
            .await
            .expect_err("concurrent refresh requires a full-row unique index");
        assert_eq!(db_error(&missing_unique_index).code().code(), "55000");
        client
            .batch_execute(
                "CREATE UNIQUE INDEX compat_mv_concurrent_partial_uq \
                 ON compat_mv_concurrent (item_id) WHERE item_id = 1",
            )
            .await
            .expect("create a partial unique index that does not cover every row");
        let partial_unique_index = client
            .batch_execute("REFRESH MATERIALIZED VIEW CONCURRENTLY compat_mv_concurrent")
            .await
            .expect_err("a partial unique index does not qualify for concurrent refresh");
        assert_eq!(db_error(&partial_unique_index).code().code(), "55000");
        client
            .batch_execute(
                "CREATE UNIQUE INDEX compat_mv_concurrent_id_uq \
                 ON compat_mv_concurrent (item_id)",
            )
            .await
            .expect("create the required unique index");
        client
            .execute(
                "UPDATE compat_mv_concurrent_source SET label = 'updated' WHERE id = 2",
                &[],
            )
            .await
            .expect("change a source row before concurrent refresh");
        client
            .batch_execute("REFRESH MATERIALIZED VIEW CONCURRENTLY compat_mv_concurrent")
            .await
            .expect("refresh materialized view concurrently after indexing");
        let catalog = client
            .query_one(
                "SELECT hasindexes, ispopulated FROM pg_matviews \
                 WHERE matviewname = 'compat_mv_concurrent'",
                &[],
            )
            .await
            .expect("inspect concurrently refreshed view state");
        let rows = client
            .query(
                "SELECT item_id, item_label FROM compat_mv_concurrent ORDER BY item_id",
                &[],
            )
            .await
            .expect("read concurrently refreshed rows");

        // Assert
        assert_eq!(db_error(&missing_unique_index).code().code(), "55000");
        assert!(catalog.get::<_, bool>(0));
        assert!(catalog.get::<_, bool>(1));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "one");
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, String>(1), "updated");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_unqualified_index_shapes_for_concurrent_materialized_view_refresh() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_concurrent_index_shapes").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_mv_index_shapes_source (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_mv_index_shapes_source VALUES (1, 'one'), (2, 'two')",
                "CREATE MATERIALIZED VIEW compat_mv_index_shapes (item_id, item_label) AS \
                   SELECT id, label FROM compat_mv_index_shapes_source WITH DATA",
                "CREATE UNIQUE INDEX compat_mv_index_shapes_partial_uq \
                   ON compat_mv_index_shapes (item_id) WHERE item_id = 1",
                "CREATE UNIQUE INDEX compat_mv_index_shapes_expression_uq \
                   ON compat_mv_index_shapes ((lower(item_label)))",
                "UPDATE compat_mv_index_shapes_source \
                   SET label = 'changed' WHERE id = 2",
            ],
        )
        .await
        .expect("create view, nonqualifying unique indexes, and a pending source change");

        // Act
        let error = client
            .batch_execute("REFRESH MATERIALIZED VIEW CONCURRENTLY compat_mv_index_shapes")
            .await
            .expect_err("partial and expression indexes do not qualify");
        let rows = client
            .query(
                "SELECT item_id, item_label FROM compat_mv_index_shapes ORDER BY item_id",
                &[],
            )
            .await
            .expect("read the old output after eligibility rejection");

        // Assert
        assert_eq!(db_error(&error).code().code(), "55000");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].get::<_, String>(1), "two");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_indexes_after_ordinary_materialized_view_refresh() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_ordinary_refresh_indexes").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_mv_ordinary_refresh_source (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_mv_ordinary_refresh_source VALUES (1, 'one')",
                "CREATE MATERIALIZED VIEW compat_mv_ordinary_refresh (item_id, item_label) AS \
                   SELECT id, label FROM compat_mv_ordinary_refresh_source WITH DATA",
                "CREATE UNIQUE INDEX compat_mv_ordinary_refresh_id_uq \
                   ON compat_mv_ordinary_refresh (item_id)",
                "UPDATE compat_mv_ordinary_refresh_source SET label = 'updated' WHERE id = 1",
            ],
        )
        .await
        .expect("create and index a populated view, then change its source");

        // Act
        client
            .batch_execute("REFRESH MATERIALIZED VIEW compat_mv_ordinary_refresh")
            .await
            .expect("refresh the view without concurrent mode");
        let catalog = client
            .query_one(
                "SELECT hasindexes, ispopulated FROM pg_matviews \
                 WHERE matviewname = 'compat_mv_ordinary_refresh'",
                &[],
            )
            .await
            .expect("inspect refreshed view state");
        let rows = client
            .query("SELECT item_label FROM compat_mv_ordinary_refresh", &[])
            .await
            .expect("read ordinary refresh output");

        // Assert
        assert!(catalog.get::<_, bool>(0));
        assert!(catalog.get::<_, bool>(1));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, String>(0), "updated");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_active_materialized_view_when_concurrent_refresh_cannot_rebuild_unique_index() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_concurrent_refresh_failure").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_mv_concurrent_failure_source (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_mv_concurrent_failure_source VALUES (1, 'one'), (2, 'two')",
                "CREATE MATERIALIZED VIEW compat_mv_concurrent_failure (item_id, item_label) AS \
                   SELECT id, label FROM compat_mv_concurrent_failure_source WITH DATA",
                "CREATE UNIQUE INDEX compat_mv_concurrent_failure_label_uq \
                   ON compat_mv_concurrent_failure (item_label)",
            ],
        )
        .await
        .expect("create populated view and its unique label index");
        client
            .execute(
                "UPDATE compat_mv_concurrent_failure_source SET label = 'one' WHERE id = 2",
                &[],
            )
            .await
            .expect("make the replacement output violate its unique index");

        // Act
        let refresh_error = client
            .batch_execute(
                "REFRESH MATERIALIZED VIEW CONCURRENTLY compat_mv_concurrent_failure",
            )
            .await
            .expect_err("reject a replacement that violates the carried unique index");
        let rows = client
            .query(
                "SELECT item_id, item_label FROM compat_mv_concurrent_failure ORDER BY item_id",
                &[],
            )
            .await
            .expect("read the previously active output after refresh failure");
        let catalog = client
            .query_one(
                "SELECT hasindexes, ispopulated FROM pg_matviews \
                 WHERE matviewname = 'compat_mv_concurrent_failure'",
                &[],
            )
            .await
            .expect("inspect the view state after refresh failure");

        // Assert
        assert_eq!(db_error(&refresh_error).code().code(), "23505");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, String>(1), "one");
        assert_eq!(rows[1].get::<_, String>(1), "two");
        assert!(catalog.get::<_, bool>(0));
        assert!(catalog.get::<_, bool>(1));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_concurrent_refresh_of_unpopulated_materialized_view() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_concurrent_refresh_unpopulated").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_mv_concurrent_unpopulated_source (id INT PRIMARY KEY)",
            )
            .await
            .expect("create source table");
        client
            .batch_execute(
                "CREATE MATERIALIZED VIEW compat_mv_concurrent_unpopulated AS \
                   SELECT id FROM compat_mv_concurrent_unpopulated_source WITH NO DATA",
            )
            .await
            .expect("create an unpopulated materialized view");

        // Act
        let refresh_error = client
            .batch_execute(
                "REFRESH MATERIALIZED VIEW CONCURRENTLY compat_mv_concurrent_unpopulated",
            )
            .await
            .expect_err("concurrent refresh requires an already populated view");

        // Assert
        assert_eq!(db_error(&refresh_error).code().code(), "55000");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_concurrent_refresh_with_no_data() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_concurrent_refresh_no_data").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_mv_concurrent_no_data_source (id INT PRIMARY KEY)")
            .await
            .expect("create source table");
        client
            .batch_execute(
                "CREATE MATERIALIZED VIEW compat_mv_concurrent_no_data AS \
                   SELECT id FROM compat_mv_concurrent_no_data_source",
            )
            .await
            .expect("create populated materialized view");

        // Act
        let refresh_error = client
            .batch_execute(
                "REFRESH MATERIALIZED VIEW CONCURRENTLY compat_mv_concurrent_no_data WITH NO DATA",
            )
            .await
            .expect_err("CONCURRENTLY cannot be combined with WITH NO DATA");

        // Assert
        assert_eq!(db_error(&refresh_error).code().code(), "42601");
        assert!(db_error(&refresh_error)
            .message()
            .contains("CONCURRENTLY and WITH NO DATA options cannot be used together"));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_skip_existing_relation_for_create_table_as_if_not_exists() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("ctas_if_not_exists").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_ctas_if_not_exists (copy_id, copy_label) AS \
                 VALUES (1, 'original')",
            )
            .await
            .expect("create initial CTAS relation");

        // Act
        let duplicate = client
            .batch_execute(
                "CREATE TABLE IF NOT EXISTS compat_ctas_if_not_exists \
                   (copy_id, copy_label) AS \
                 SELECT copy_id, copy_label FROM compat_ctas_missing_source",
            )
            .await;
        let rows = if duplicate.is_ok() {
            Some(
                client
                    .query(
                        "SELECT copy_id, copy_label FROM compat_ctas_if_not_exists \
                         ORDER BY copy_id",
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
            duplicate.is_ok(),
            "CTAS IF NOT EXISTS should skip the existing relation: {duplicate:?}"
        );
        let rows = rows
            .expect("successful duplicate CTAS should leave the relation queryable")
            .expect("read existing CTAS relation");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "original");
    });
}

fn assert_virtual_generated_column_contract(
    values: &[tokio_postgres::Row],
    generated: &[tokio_postgres::Row],
    explicit_value: &tokio_postgres::Error,
) {
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].get::<_, i32>(0), 4);
    assert_eq!(values[0].get::<_, i32>(1), 8);
    assert_eq!(values[0].get::<_, i32>(2), 5);
    assert_eq!(generated.len(), 3);
    assert_eq!(generated[0].get::<_, String>(1), "");
    assert_eq!(generated[1].get::<_, String>(0), "doubled");
    assert_eq!(generated[1].get::<_, String>(1), "v");
    assert_eq!(generated[2].get::<_, String>(0), "incremented");
    assert_eq!(generated[2].get::<_, String>(1), "v");
    assert_eq!(db_error(explicit_value).code().code(), "428C9");
}

#[test]
fn should_compute_explicit_implicit_virtual_generated_columns() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("virtual_generated_columns").await;
        let (client, connection) = server.connect().await;
        let created = client
            .batch_execute(
                "CREATE TABLE compat_virtual_generated (\
                   base INT,\
                   doubled INT GENERATED ALWAYS AS (base * 2) VIRTUAL,\
                   incremented INT GENERATED ALWAYS AS (base + 1)\
                 )",
            )
            .await;
        let inserted = if created.is_ok() {
            Some(
                client
                    .execute("INSERT INTO compat_virtual_generated (base) VALUES (4)", &[])
                    .await,
            )
        } else {
            None
        };
        let values = if inserted.as_ref().is_some_and(Result::is_ok) {
            Some(
                client
                    .query(
                        "SELECT base, doubled, incremented \
                         FROM compat_virtual_generated",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };
        let generated = if created.is_ok() {
            Some(
                client
                    .query(
                        "SELECT attname, attgenerated::TEXT \
                         FROM pg_attribute \
                         WHERE attrelid = 'compat_virtual_generated'::regclass \
                           AND attnum > 0 ORDER BY attnum",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };

        // Act
        let explicit_value = if inserted.as_ref().is_some_and(Result::is_ok) {
            Some(
                client
                    .execute(
                        "INSERT INTO compat_virtual_generated (base, doubled) VALUES (3, 6)",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };

        // Assert
        let create_sqlstate = created
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code());
        drop(client);
        server.shutdown(connection).await;
        assert!(
            created.is_ok(),
            "PostgreSQL 18 VIRTUAL generated columns should be accepted; SQLSTATE: {create_sqlstate:?}"
        );
        inserted
            .expect("insert should be attempted after table creation")
            .expect("insert into generated-column table");
        let values = values
            .expect("read computed values after insert")
            .expect("query generated-column values");
        let generated = generated
            .expect("inspect virtual generated-column metadata")
            .expect("read generated-column metadata");
        let explicit_value = explicit_value
            .expect("attempt explicit assignment to a virtual generated column")
            .expect_err("a virtual generated column only accepts DEFAULT");
        assert_virtual_generated_column_contract(&values, &generated, &explicit_value);
    });
}

#[test]
#[ignore = "Pending #839 materialized-view rename acceptance; #833 remains a prerequisite"]
fn should_track_materialized_view_rename_in_pg_matviews() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_rename_catalog").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE MATERIALIZED VIEW compat_mv_rename AS SELECT 1 AS view_id")
            .await
            .expect("create materialized view before renaming");

        // Act
        client
            .batch_execute(
                "ALTER MATERIALIZED VIEW compat_mv_rename \
                 RENAME TO compat_mv_renamed",
            )
            .await
            .expect("rename materialized view");
        let catalog = client
            .query_one(
                "SELECT matviewname, ispopulated FROM pg_matviews \
                 WHERE matviewname = 'compat_mv_renamed'",
                &[],
            )
            .await
            .expect("inspect renamed materialized-view catalog row");

        // Assert
        assert_eq!(catalog.get::<_, String>(0), "compat_mv_renamed");
        assert!(catalog.get::<_, bool>(1));

        drop(client);
        server.shutdown(connection).await;
    });
}
