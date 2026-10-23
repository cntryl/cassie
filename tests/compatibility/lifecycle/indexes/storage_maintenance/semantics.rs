#[test]
fn should_store_btree_deduplicate_items_option() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("btree_deduplicate_items_option").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_btree_deduplicate_rows (payload TEXT)",
                "CREATE INDEX compat_btree_deduplicate_idx \
                   ON compat_btree_deduplicate_rows USING btree (payload) \
                   WITH (deduplicate_items = off)",
            ],
        )
        .await
        .expect("create a btree index with deduplication disabled");

        // Act
        let options: Option<String> = client
            .query_one(
                "SELECT reloptions::text FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_btree_deduplicate_idx'::regclass",
                &[],
            )
            .await
            .expect("read btree storage parameters from the relation catalog")
            .try_get(0)
            .expect("index storage options");

        // Assert
        assert_eq!(options.as_deref(), Some("{deduplicate_items=off}"));

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_reject_expression_index_statistics_until_index_catalog_support_is_selected() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("expression_index_statistics_target").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_expression_statistics_rows (payload TEXT)",
                "CREATE INDEX compat_expression_statistics_idx \
                   ON compat_expression_statistics_rows (lower(payload))",
            ],
        )
        .await
        .expect("create a btree index over a text expression");

        // Act
        let rejected = client
            .batch_execute(
                "ALTER INDEX compat_expression_statistics_idx \
                 ALTER COLUMN 1 SET STATISTICS 23",
            )
            .await
            .expect_err("index expression statistics are not in the selected catalog scope");

        // Assert
        assert_eq!(db_error(&rejected).code().code(), "42601");

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_ignore_alter_index_if_exists_for_a_missing_index() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("alter_index_if_exists_missing").await;
        let (client, connection) = server.connect().await;

        // Act
        client
            .batch_execute(
                "ALTER INDEX IF EXISTS compat_missing_alter_index \
                   SET (fillfactor = 81)",
            )
            .await
            .expect("skip SET when the index is absent");
        client
            .batch_execute(
                "ALTER INDEX IF EXISTS compat_missing_alter_index \
                   RESET (fillfactor)",
            )
            .await
            .expect("skip RESET when the index is absent");
        let remains_missing: bool = client
            .query_one(
                "SELECT pg_catalog.to_regclass('compat_missing_alter_index') IS NULL",
                &[],
            )
            .await
            .expect("check that the missing index was not created")
            .try_get(0)
            .expect("missing index state");

        // Assert
        assert!(remains_missing);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_reject_reindex_schema_until_the_scope_is_selected() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reindex_named_schema_scope").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_reindex_selected",
                "CREATE TABLE compat_reindex_selected.rows (id INT, payload TEXT)",
                "CREATE INDEX compat_reindex_selected_idx \
                   ON compat_reindex_selected.rows (payload)",
            ],
        )
        .await
        .expect("create a scalar index in the schema");

        // Act
        let rejected = client
            .batch_execute("REINDEX SCHEMA compat_reindex_selected")
            .await
            .expect_err("schema-wide REINDEX is outside the selected contract");

        // Assert
        assert_eq!(db_error(&rejected).code().code(), "0A000");

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_reject_gist_buffering_option_until_the_method_is_supported() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("gist_buffering_storage_option").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_gist_buffering_rows (value INT)")
            .await
            .expect("create a table for the unsupported GiST method");

        // Act
        let rejected = client
            .batch_execute(
                "CREATE INDEX compat_gist_buffering_idx \
                   ON compat_gist_buffering_rows USING gist (value) \
                   WITH (buffering = off)",
            )
            .await
            .expect_err("GiST buffering must not imply support for the GiST method");
        let index_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_indexes \
                 WHERE indexname = 'compat_gist_buffering_idx'",
                &[],
            )
            .await
            .expect("check that the rejected GiST index was not published")
            .try_get(0)
            .expect("index count");

        // Assert
        assert_eq!(db_error(&rejected).code().code(), "0A000");
        assert_eq!(index_count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_reject_brin_pages_per_range_until_the_method_is_supported() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("brin_pages_per_range_option").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_brin_pages_rows (value INT)")
            .await
            .expect("create a table for the unsupported BRIN method");

        // Act
        let rejected = client
            .batch_execute(
                "CREATE INDEX compat_brin_pages_idx \
                   ON compat_brin_pages_rows USING brin (value) \
                   WITH (pages_per_range = 16)",
            )
            .await
            .expect_err("BRIN options must not imply support for the BRIN method");
        let index_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_indexes \
                 WHERE indexname = 'compat_brin_pages_idx'",
                &[],
            )
            .await
            .expect("check that the rejected BRIN index was not published")
            .try_get(0)
            .expect("index count");

        // Assert
        assert_eq!(db_error(&rejected).code().code(), "0A000");
        assert_eq!(index_count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_reject_analyze_column_lists_until_statistics_are_selected() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("analyze_multiple_tables_columns").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_analyze_multi_first (id INT, label TEXT, ignored TEXT)",
            )
            .await
            .expect("create the first table");
        client
            .batch_execute(
                "CREATE TABLE compat_analyze_multi_second (id INT, label TEXT, ignored TEXT)",
            )
            .await
            .expect("create the second table");

        // Act
        let rejected = client
            .batch_execute(
                "ANALYZE (VERBOSE FALSE, SKIP_LOCKED FALSE, \
                   BUFFER_USAGE_LIMIT '1MB') \
                   compat_analyze_multi_first (label), \
                   compat_analyze_multi_second (label)",
            )
            .await
            .expect_err("PostgreSQL ANALYZE statistics are not selected");

        // Assert
        assert_eq!(db_error(&rejected).code().code(), "0A000");

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_reject_multi_table_vacuum_until_heap_maintenance_is_selected() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("vacuum_multiple_tables_remaining_options").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_vacuum_multi_first \
                   (id INT PRIMARY KEY, label TEXT)",
                "CREATE TABLE compat_vacuum_multi_second \
                   (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_vacuum_multi_first VALUES \
                   (1, 'common'), (2, 'common'), (3, 'rare')",
                "INSERT INTO compat_vacuum_multi_second VALUES \
                   (1, 'common'), (2, 'common'), (3, 'rare')",
                "DELETE FROM compat_vacuum_multi_first WHERE id = 3",
                "DELETE FROM compat_vacuum_multi_second WHERE id = 3",
            ],
        )
        .await
        .expect("create two indexed tables with dead tuples and skewed values");

        // Act
        let rejected = client
            .batch_execute(
                "VACUUM (ANALYZE, FREEZE, VERBOSE FALSE, DISABLE_PAGE_SKIPPING TRUE, \
                   SKIP_LOCKED FALSE, INDEX_CLEANUP AUTO, PROCESS_MAIN TRUE, \
                   PROCESS_TOAST TRUE, TRUNCATE FALSE, PARALLEL 0, \
                   BUFFER_USAGE_LIMIT '1MB') \
                   compat_vacuum_multi_first (label), \
                   compat_vacuum_multi_second (label)",
            )
            .await
            .expect_err("PostgreSQL VACUUM and ANALYZE are not selected");
        let visible_count: i64 = client
            .query_one(
                "SELECT (SELECT count(*) FROM compat_vacuum_multi_first) + \
                        (SELECT count(*) FROM compat_vacuum_multi_second)",
                &[],
            )
            .await
            .expect("confirm rejected maintenance leaves visible rows unchanged")
            .try_get(0)
            .expect("visible row count");

        // Assert
        assert_eq!(db_error(&rejected).code().code(), "0A000");
        assert_eq!(visible_count, 4);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_reject_legacy_vacuum_freeze_verbose_form() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("legacy_vacuum_freeze_verbose").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_legacy_vacuum_rows (id INT PRIMARY KEY)",
                "INSERT INTO compat_legacy_vacuum_rows VALUES (1), (2)",
            ],
        )
        .await
        .expect("create rows for the legacy VACUUM form");

        // Act
        let rejected = client
            .batch_execute("VACUUM FREEZE VERBOSE compat_legacy_vacuum_rows")
            .await
            .expect_err("legacy PostgreSQL VACUUM syntax is not selected");
        let row_count: i64 = client
            .query_one("SELECT count(*) FROM compat_legacy_vacuum_rows", &[])
            .await
            .expect("confirm rejected maintenance leaves rows unchanged")
            .try_get(0)
            .expect("row count");

        // Assert
        assert_eq!(db_error(&rejected).code().code(), "0A000");
        assert_eq!(row_count, 2);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_reject_skip_database_statistics_vacuum_option() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("vacuum_skip_database_statistics").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_vacuum_skip_db_stats_rows (id INT)",
                "INSERT INTO compat_vacuum_skip_db_stats_rows VALUES (1), (2)",
                "DELETE FROM compat_vacuum_skip_db_stats_rows WHERE id = 2",
            ],
        )
        .await
        .expect("create a table with one visible row");

        // Act
        let rejected = client
            .batch_execute("VACUUM (SKIP_DATABASE_STATS) compat_vacuum_skip_db_stats_rows")
            .await
            .expect_err("VACUUM options are not selected");
        let row_count: i64 = client
            .query_one("SELECT count(*) FROM compat_vacuum_skip_db_stats_rows", &[])
            .await
            .expect("confirm rejected maintenance leaves rows unchanged")
            .try_get(0)
            .expect("row count");

        // Assert
        assert_eq!(db_error(&rejected).code().code(), "0A000");
        assert_eq!(row_count, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_reject_database_only_vacuum_option() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("vacuum_only_database_statistics").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_vacuum_only_db_stats_rows (id INT)")
            .await
            .expect("create a table for the unsupported VACUUM option");

        // Act
        let rejected = client
            .batch_execute("VACUUM (ONLY_DATABASE_STATS)")
            .await
            .expect_err("database-only VACUUM is not selected");
        let row_count: i64 = client
            .query_one("SELECT count(*) FROM compat_vacuum_only_db_stats_rows", &[])
            .await
            .expect("confirm rejected maintenance leaves rows unchanged")
            .try_get(0)
            .expect("row count");

        // Assert
        assert_eq!(db_error(&rejected).code().code(), "0A000");
        assert_eq!(row_count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_reject_gin_pending_list_cleanup_without_a_gin_index() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("gin_pending_list_cleanup_effect").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_gin_pending_rows (id INT, value INT)",
            )
            .await
            .expect("create a table for the unsupported GIN cleanup");
        client
            .batch_execute(
                "CREATE INDEX compat_gin_pending_idx \
                   ON compat_gin_pending_rows (value)",
            )
            .await
            .expect("create a supported btree index");
        client
            .batch_execute("INSERT INTO compat_gin_pending_rows VALUES (1, 10)")
            .await
            .expect("insert a row without creating a GIN pending list");

        // Act
        let rejected = client
            .query_one(
                "SELECT gin_clean_pending_list('compat_gin_pending_idx'::regclass)",
                &[],
            )
            .await
            .expect_err("GIN pending-list cleanup must reject a non-GIN index");
        let row_count: i64 = client
            .query_one("SELECT count(*) FROM compat_gin_pending_rows", &[])
            .await
            .expect("count rows after the rejected cleanup")
            .try_get(0)
            .expect("row count");

        // Assert
        assert_eq!(db_error(&rejected).code().code(), "42601");
        assert_eq!(row_count, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
