async fn read_reindex_schema_storage(client: &tokio_postgres::Client) -> Vec<tokio_postgres::Row> {
    client
        .query(
            "SELECT namespace.nspname, relation.relname, \
                    relation.relfilenode::bigint \
             FROM pg_catalog.pg_class AS relation \
             JOIN pg_catalog.pg_namespace AS namespace \
               ON namespace.oid = relation.relnamespace \
             WHERE (namespace.nspname = 'compat_reindex_selected' \
                    AND relation.relname IN ('one_idx', 'two_idx')) \
                OR (namespace.nspname = 'compat_reindex_outside' \
                    AND relation.relname = 'outside_idx') \
             ORDER BY namespace.nspname, relation.relname",
            &[],
        )
        .await
        .expect("read index storage identities")
}

fn index_storage_nodes(rows: &[tokio_postgres::Row]) -> Vec<(String, String, i64)> {
    rows.iter()
        .map(|row| {
            (
                row.get::<_, String>(0),
                row.get::<_, String>(1),
                row.get::<_, i64>(2),
            )
        })
        .collect()
}

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
fn should_set_expression_index_statistics_target() {
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
        client
            .batch_execute(
                "ALTER INDEX compat_expression_statistics_idx \
                 ALTER COLUMN 1 SET STATISTICS 23",
            )
            .await
            .expect("set the statistics target for the index expression");
        let target: i32 = client
            .query_one(
                "SELECT attstattarget FROM pg_catalog.pg_attribute \
                 WHERE attrelid = 'compat_expression_statistics_idx'::regclass \
                   AND attnum = 1",
                &[],
            )
            .await
            .expect("read the index expression statistics target")
            .try_get(0)
            .expect("expression statistics target");

        // Assert
        assert_eq!(target, 23);

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
                   SET (fillfactor = 81); \
                 ALTER INDEX IF EXISTS compat_missing_alter_index \
                   RESET (fillfactor)",
            )
            .await
            .expect("skip both ALTER INDEX operations when the index is absent");
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
fn should_rebuild_only_indexes_in_the_named_schema() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reindex_named_schema_scope").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_reindex_selected",
                "CREATE SCHEMA compat_reindex_outside",
                "CREATE TABLE compat_reindex_selected.first_rows (id INT, payload TEXT)",
                "CREATE TABLE compat_reindex_selected.second_rows (id INT, payload TEXT)",
                "CREATE TABLE compat_reindex_outside.outside_rows (id INT, payload TEXT)",
                "INSERT INTO compat_reindex_selected.first_rows VALUES \
                   (2, 'two'), (1, 'one')",
                "INSERT INTO compat_reindex_selected.second_rows VALUES \
                   (4, 'four'), (3, 'three')",
                "INSERT INTO compat_reindex_outside.outside_rows VALUES \
                   (6, 'six'), (5, 'five')",
                "CREATE INDEX one_idx ON compat_reindex_selected.first_rows (payload)",
                "CREATE INDEX two_idx ON compat_reindex_selected.second_rows (payload)",
                "CREATE INDEX outside_idx ON compat_reindex_outside.outside_rows (payload)",
            ],
        )
        .await
        .expect("create indexed tables in the selected and unrelated schemas");
        let before = read_reindex_schema_storage(&client).await;

        // Act
        client
            .batch_execute("REINDEX SCHEMA compat_reindex_selected")
            .await
            .expect("rebuild the indexes in the selected schema");
        let after = read_reindex_schema_storage(&client).await;
        let table_rows = client
            .query(
                "SELECT id, payload FROM compat_reindex_selected.first_rows \
                 UNION ALL \
                 SELECT id, payload FROM compat_reindex_selected.second_rows \
                 ORDER BY id",
                &[],
            )
            .await
            .expect("read selected-schema rows after reindexing");
        let validity = client
            .query(
                "SELECT index_class.relname, index_state.indisvalid \
                 FROM pg_catalog.pg_index AS index_state \
                 JOIN pg_catalog.pg_class AS index_class \
                   ON index_class.oid = index_state.indexrelid \
                 JOIN pg_catalog.pg_namespace AS namespace \
                   ON namespace.oid = index_class.relnamespace \
                 WHERE namespace.nspname = 'compat_reindex_selected' \
                 ORDER BY index_class.relname",
                &[],
            )
            .await
            .expect("read selected index validity after reindexing");

        // Assert
        let before = index_storage_nodes(&before);
        let after = index_storage_nodes(&after);
        assert_eq!(before.len(), 3);
        assert_eq!(after.len(), 3);
        for (before, after) in before.iter().zip(&after) {
            assert_eq!((&before.0, &before.1), (&after.0, &after.1));
            if before.0 == "compat_reindex_selected" {
                assert_ne!(before.2, after.2, "selected index should be rebuilt");
            } else {
                assert_eq!(before.2, after.2, "unselected schema index should remain");
            }
        }
        assert_eq!(
            table_rows
                .iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![
                (1, "one".to_owned()),
                (2, "two".to_owned()),
                (3, "three".to_owned()),
                (4, "four".to_owned()),
            ]
        );
        assert_eq!(validity.len(), 2);
        assert!(validity.iter().all(|row| row.get::<_, bool>(1)));

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_store_gist_buffering_option() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("gist_buffering_storage_option").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_gist_buffering_rows (document TSVECTOR)",
                "CREATE INDEX compat_gist_buffering_idx \
                   ON compat_gist_buffering_rows USING gist (document) \
                   WITH (buffering = off)",
            ],
        )
        .await
        .expect("create a GiST tsvector index with buffering disabled");

        // Act
        let options: Option<String> = client
            .query_one(
                "SELECT reloptions::text FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_gist_buffering_idx'::regclass",
                &[],
            )
            .await
            .expect("read the GiST storage parameter")
            .try_get(0)
            .expect("GiST storage options");

        // Assert
        assert_eq!(options.as_deref(), Some("{buffering=off}"));

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_store_brin_pages_per_range_option() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("brin_pages_per_range_option").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_brin_pages_rows (value INT)",
                "CREATE INDEX compat_brin_pages_idx \
                   ON compat_brin_pages_rows USING brin (value) \
                   WITH (pages_per_range = 16)",
            ],
        )
        .await
        .expect("create a BRIN index with a nondefault range size");

        // Act
        let options: Option<String> = client
            .query_one(
                "SELECT reloptions::text FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_brin_pages_idx'::regclass",
                &[],
            )
            .await
            .expect("read the BRIN storage parameter")
            .try_get(0)
            .expect("BRIN storage options");

        // Assert
        assert_eq!(options.as_deref(), Some("{pages_per_range=16}"));

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_analyze_multiple_tables_with_column_lists() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("analyze_multiple_tables_columns").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_analyze_multi_first (id INT, label TEXT, ignored TEXT)",
                "CREATE TABLE compat_analyze_multi_second (id INT, label TEXT, ignored TEXT)",
                "INSERT INTO compat_analyze_multi_first VALUES \
                   (1, 'common', 'unused'), (2, 'common', 'unused'), (3, 'rare', 'unused')",
                "INSERT INTO compat_analyze_multi_second VALUES \
                   (1, 'common', 'unused'), (2, 'common', 'unused'), (3, 'rare', 'unused')",
            ],
        )
        .await
        .expect("create two tables with independent column distributions");

        // Act
        client
            .batch_execute(
                "ANALYZE (VERBOSE FALSE, SKIP_LOCKED FALSE, \
                   BUFFER_USAGE_LIMIT '1MB') \
                   compat_analyze_multi_first (label), \
                   compat_analyze_multi_second (label)",
            )
            .await
            .expect("analyze selected columns across multiple tables");
        let statistics = client
            .query(
                "SELECT tablename, attname, most_common_vals::text \
                 FROM pg_catalog.pg_stats \
                 WHERE schemaname = 'public' \
                   AND tablename IN ('compat_analyze_multi_first', \
                                     'compat_analyze_multi_second') \
                 ORDER BY tablename, attname",
                &[],
            )
            .await
            .expect("read statistics produced by the multi-table ANALYZE");

        // Assert
        assert_eq!(statistics.len(), 2);
        assert_eq!(
            statistics[0].get::<_, String>(0),
            "compat_analyze_multi_first"
        );
        assert_eq!(statistics[0].get::<_, String>(1), "label");
        assert_eq!(
            statistics[0].get::<_, Option<String>>(2).as_deref(),
            Some("{common}")
        );
        assert_eq!(
            statistics[1].get::<_, String>(0),
            "compat_analyze_multi_second"
        );
        assert_eq!(statistics[1].get::<_, String>(1), "label");
        assert_eq!(
            statistics[1].get::<_, Option<String>>(2).as_deref(),
            Some("{common}")
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_vacuum_multiple_tables_with_remaining_options() {
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
        client
            .batch_execute(
                "VACUUM (ANALYZE, FREEZE, VERBOSE FALSE, DISABLE_PAGE_SKIPPING TRUE, \
                   SKIP_LOCKED FALSE, INDEX_CLEANUP AUTO, PROCESS_MAIN TRUE, \
                   PROCESS_TOAST TRUE, TRUNCATE FALSE, PARALLEL 0, \
                   BUFFER_USAGE_LIMIT '1MB') \
                   compat_vacuum_multi_first (label), \
                   compat_vacuum_multi_second (label)",
            )
            .await
            .expect("vacuum and analyze multiple selected table columns with PostgreSQL options");
        let states = client
            .query(
                "SELECT stats.relname, stats.last_vacuum IS NOT NULL, \
                        stats.last_analyze IS NOT NULL, stats.n_dead_tup, \
                        column_stats.attname, column_stats.most_common_vals::text \
                 FROM pg_catalog.pg_stat_user_tables AS stats \
                 JOIN pg_catalog.pg_stats AS column_stats \
                   ON column_stats.schemaname = stats.schemaname \
                  AND column_stats.tablename = stats.relname \
                  AND column_stats.attname = 'label' \
                 WHERE stats.schemaname = 'public' \
                   AND stats.relname IN ('compat_vacuum_multi_first', \
                                         'compat_vacuum_multi_second') \
                 ORDER BY stats.relname",
                &[],
            )
            .await
            .expect("read per-table vacuum and selected-column statistics");
        let visible_rows = client
            .query(
                "SELECT id FROM compat_vacuum_multi_first \
                 UNION ALL \
                 SELECT id FROM compat_vacuum_multi_second \
                 ORDER BY id",
                &[],
            )
            .await
            .expect("read rows after multi-table vacuum");

        // Assert
        assert_eq!(states.len(), 2);
        for state in &states {
            assert!(state.get::<_, bool>(1));
            assert!(state.get::<_, bool>(2));
            assert_eq!(state.get::<_, i64>(3), 0);
            assert_eq!(state.get::<_, String>(4), "label");
            assert_eq!(
                state.get::<_, Option<String>>(5).as_deref(),
                Some("{common}")
            );
        }
        assert_eq!(
            visible_rows
                .iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![1, 1, 2, 2]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_accept_legacy_vacuum_freeze_verbose_form() {
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
        client
            .batch_execute("VACUUM FREEZE VERBOSE compat_legacy_vacuum_rows")
            .await
            .expect("vacuum the table with legacy FREEZE and VERBOSE syntax");
        let state = client
            .query_one(
                "SELECT last_vacuum IS NOT NULL, n_dead_tup \
                 FROM pg_catalog.pg_stat_user_tables \
                 WHERE schemaname = 'public' \
                   AND relname = 'compat_legacy_vacuum_rows'",
                &[],
            )
            .await
            .expect("read legacy VACUUM effects");

        // Assert
        assert!(state.get::<_, bool>(0));
        assert_eq!(state.get::<_, i64>(1), 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_skip_database_statistics_for_table_vacuum() {
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
        .expect("create a table with one dead tuple");

        // Act
        client
            .batch_execute("VACUUM (SKIP_DATABASE_STATS) compat_vacuum_skip_db_stats_rows")
            .await
            .expect("vacuum the table while skipping database-wide statistics");
        let state = client
            .query_one(
                "SELECT last_vacuum IS NOT NULL, n_dead_tup \
                 FROM pg_catalog.pg_stat_user_tables \
                 WHERE schemaname = 'public' \
                   AND relname = 'compat_vacuum_skip_db_stats_rows'",
                &[],
            )
            .await
            .expect("read per-table vacuum effects");

        // Assert
        assert!(state.get::<_, bool>(0));
        assert_eq!(state.get::<_, i64>(1), 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_update_only_database_statistics_without_vacuuming_tables() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("vacuum_only_database_statistics").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_vacuum_only_db_stats_rows (id INT)")
            .await
            .expect("create a table that has not been vacuumed");

        // Act
        client
            .batch_execute("VACUUM (ONLY_DATABASE_STATS)")
            .await
            .expect("update only database-wide statistics");
        let last_vacuum_is_null: bool = client
            .query_one(
                "SELECT last_vacuum IS NULL \
                 FROM pg_catalog.pg_stat_user_tables \
                 WHERE schemaname = 'public' \
                   AND relname = 'compat_vacuum_only_db_stats_rows'",
                &[],
            )
            .await
            .expect("read table maintenance state after database-only vacuum")
            .try_get(0)
            .expect("last vacuum state");

        // Assert
        assert!(last_vacuum_is_null);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_reclaim_gin_pending_list_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("gin_pending_list_cleanup_effect").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_gin_pending_rows (id INT, document TSVECTOR); \
                 CREATE INDEX compat_gin_pending_idx \
                   ON compat_gin_pending_rows USING gin (document) \
                   WITH (fastupdate = on, gin_pending_list_limit = 8192)",
            )
            .await
            .expect("create a GIN index with pending-list insertion enabled");
        let values = (0..1_000)
            .map(|id| format!("({id}, 'alpha:1 beta:2'::tsvector)"))
            .collect::<Vec<_>>()
            .join(", ");
        client
            .batch_execute(&format!(
                "INSERT INTO compat_gin_pending_rows (id, document) VALUES {values}"
            ))
            .await
            .expect("insert enough indexed rows to populate the GIN pending list");

        // Act
        let first_cleanup = client
            .query_one(
                "SELECT gin_clean_pending_list('compat_gin_pending_idx'::regclass)",
                &[],
            )
            .await
            .expect("clean the populated GIN pending list");
        let second_cleanup = client
            .query_one(
                "SELECT gin_clean_pending_list('compat_gin_pending_idx'::regclass)",
                &[],
            )
            .await
            .expect("confirm the GIN pending list stays empty");
        let row_count: i64 = client
            .query_one("SELECT count(*) FROM compat_gin_pending_rows", &[])
            .await
            .expect("count the indexed rows after pending-list cleanup")
            .try_get(0)
            .expect("row count");

        // Assert
        assert_eq!(first_cleanup.columns()[0].type_().oid(), 20);
        assert!(first_cleanup.get::<_, i64>(0) > 0);
        assert_eq!(second_cleanup.get::<_, i64>(0), 0);
        assert_eq!(row_count, 1_000);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_observe_brin_range_maintenance() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("brin_range_maintenance_effect").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_brin_range_rows (value INT); \
                 CREATE INDEX compat_brin_range_idx \
                   ON compat_brin_range_rows USING brin (value) \
                   WITH (pages_per_range = 1)",
            )
            .await
            .expect("create a BRIN index with one-page ranges");
        let values = (1..=500)
            .map(|value| format!("({value})"))
            .collect::<Vec<_>>()
            .join(", ");
        client
            .batch_execute(&format!(
                "INSERT INTO compat_brin_range_rows (value) VALUES {values}"
            ))
            .await
            .expect("insert rows that require BRIN range summaries");

        // Act
        let first_summary = client
            .query_one(
                "SELECT brin_summarize_new_values('compat_brin_range_idx'::regclass)",
                &[],
            )
            .await
            .expect("summarize newly populated BRIN ranges");
        let repeated_summary = client
            .query_one(
                "SELECT brin_summarize_new_values('compat_brin_range_idx'::regclass)",
                &[],
            )
            .await
            .expect("confirm that already summarized BRIN ranges are unchanged");
        let existing_range = client
            .query_one(
                "SELECT brin_summarize_range('compat_brin_range_idx'::regclass, 0)",
                &[],
            )
            .await
            .expect("inspect a range that was already summarized");
        let desummarized_range = client
            .query_one(
                "SELECT brin_desummarize_range('compat_brin_range_idx'::regclass, 0)",
                &[],
            )
            .await
            .expect("remove the summary for the selected BRIN range");
        let rebuilt_range = client
            .query_one(
                "SELECT brin_summarize_range('compat_brin_range_idx'::regclass, 0)",
                &[],
            )
            .await
            .expect("rebuild the desummarized BRIN range");
        let row_count: i64 = client
            .query_one("SELECT count(*) FROM compat_brin_range_rows", &[])
            .await
            .expect("count table rows after BRIN maintenance")
            .try_get(0)
            .expect("row count");

        // Assert
        assert_eq!(first_summary.columns()[0].type_().oid(), 23);
        assert!(first_summary.get::<_, i32>(0) > 0);
        assert_eq!(repeated_summary.get::<_, i32>(0), 0);
        assert_eq!(existing_range.get::<_, i32>(0), 0);
        assert_eq!(desummarized_range.columns()[0].type_().oid(), 2278);
        assert_eq!(rebuilt_range.get::<_, i32>(0), 1);
        assert_eq!(row_count, 500);

        drop(client);
        server.shutdown(connection).await;
    });
}
