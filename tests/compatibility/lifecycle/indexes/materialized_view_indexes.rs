use super::*;

#[test]
fn should_create_unique_index_on_populated_sql_materialized_view() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_unique_index").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_indexed_mv_source (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_indexed_mv_source VALUES (1, 'one'), (2, 'two')",
                "CREATE MATERIALIZED VIEW compat_indexed_mv (item_id, item_label) AS \
                   SELECT id, label FROM compat_indexed_mv_source WITH DATA",
            ],
        )
        .await
        .expect("create and populate a SQL materialized view");

        // Act
        client
            .batch_execute(
                "CREATE UNIQUE INDEX compat_indexed_mv_id_uq \
                 ON compat_indexed_mv (item_id)",
            )
            .await
            .expect("create a unique scalar index on materialized-view output");
        let materialized_view = client
            .query_one(
                "SELECT hasindexes FROM pg_catalog.pg_matviews \
                 WHERE matviewname = 'compat_indexed_mv'",
                &[],
            )
            .await
            .expect("inspect materialized-view index state");
        let index = client
            .query_one(
                "SELECT schemaname, tablename, indexdef \
                 FROM pg_catalog.pg_indexes \
                 WHERE indexname = 'compat_indexed_mv_id_uq'",
                &[],
            )
            .await
            .expect("inspect the index attached to the logical materialized view");
        let index_identity = client
            .query_one(
                "SELECT indrelid, indisunique \
                 FROM pg_catalog.pg_index \
                 WHERE indexrelid = 'compat_indexed_mv_id_uq'",
                &[],
            )
            .await
            .expect("inspect the unique index relation identity");

        // Assert
        assert!(materialized_view.get::<_, bool>(0));
        assert_eq!(index.get::<_, String>(1), "compat_indexed_mv");
        assert!(index
            .get::<_, String>(2)
            .contains("CREATE UNIQUE INDEX compat_indexed_mv_id_uq"));
        assert_eq!(index_identity.get::<_, String>(0), "compat_indexed_mv");
        assert!(index_identity.get::<_, bool>(1));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_remove_unique_index_from_sql_materialized_view_catalog() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("drop_materialized_view_index").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_drop_mv_source (id INT PRIMARY KEY)",
                "INSERT INTO compat_drop_mv_source VALUES (1), (2)",
                "CREATE MATERIALIZED VIEW compat_drop_mv (item_id) AS \
                   SELECT id FROM compat_drop_mv_source WITH DATA",
                "CREATE UNIQUE INDEX compat_drop_mv_id_uq ON compat_drop_mv (item_id)",
            ],
        )
        .await
        .expect("create a unique index on populated materialized-view output");

        // Act
        client
            .batch_execute("DROP INDEX compat_drop_mv_id_uq")
            .await
            .expect("drop the index through its logical materialized-view relation");
        let has_indexes: bool = client
            .query_one(
                "SELECT hasindexes FROM pg_catalog.pg_matviews \
                 WHERE matviewname = 'compat_drop_mv'",
                &[],
            )
            .await
            .expect("inspect materialized-view index state after drop")
            .try_get(0)
            .expect("hasindexes value");
        let index_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_indexes \
                 WHERE indexname = 'compat_drop_mv_id_uq'",
                &[],
            )
            .await
            .expect("check the dropped index catalog row")
            .try_get(0)
            .expect("index count");

        // Assert
        // pg_matviews.hasindexes mirrors relhasindex, which PostgreSQL leaves set
        // after the last index is dropped until maintenance clears it (see
        // CollectionMeta::has_ever_had_indexes).
        assert!(has_indexes);
        assert_eq!(index_count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_unique_index_when_materialized_view_has_duplicate_keys() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("duplicate_materialized_view_index").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_duplicate_mv_source (id INT, label TEXT)",
                "INSERT INTO compat_duplicate_mv_source VALUES (1, 'one'), (1, 'duplicate')",
                "CREATE MATERIALIZED VIEW compat_duplicate_mv (item_id, item_label) AS \
                   SELECT id, label FROM compat_duplicate_mv_source WITH DATA",
            ],
        )
        .await
        .expect("create a populated materialized view with duplicate keys");

        // Act
        let rejected = client
            .batch_execute(
                "CREATE UNIQUE INDEX compat_duplicate_mv_id_uq \
                 ON compat_duplicate_mv (item_id)",
            )
            .await
            .expect_err("unique index creation must validate existing view rows");
        let indexes: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_indexes \
                 WHERE indexname = 'compat_duplicate_mv_id_uq'",
                &[],
            )
            .await
            .expect("check that the failed index was not published")
            .try_get(0)
            .expect("index count");
        let materialized_view_has_indexes: bool = client
            .query_one(
                "SELECT hasindexes FROM pg_catalog.pg_matviews \
                 WHERE matviewname = 'compat_duplicate_mv'",
                &[],
            )
            .await
            .expect("inspect materialized-view index state")
            .try_get(0)
            .expect("hasindexes value");

        // Assert
        assert_eq!(db_error(&rejected).code().code(), "23505");
        assert_eq!(indexes, 0);
        assert!(!materialized_view_has_indexes);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_seek_the_published_materialized_view_output_for_each_point_lookup() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("seek_materialized_view").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE mv_seek_source (k INT, label TEXT)",
                "INSERT INTO mv_seek_source VALUES (1, 'one'), (2, 'two')",
                "CREATE MATERIALIZED VIEW mv_seek AS SELECT k, label FROM mv_seek_source",
                "CREATE UNIQUE INDEX mv_seek_key ON mv_seek (k)",
            ],
        )
        .await
        .expect("create indexed materialized output");

        // Act
        for (key, expected) in [(1_i32, Some("one")), (3, None)] {
            let before = server.cassie.metrics();
            let rows = client
                .query("SELECT label FROM mv_seek WHERE k = $1", &[&key])
                .await
                .expect("point lookup");
            let after = server.cassie.metrics();

            // Assert
            let labels: Vec<String> = rows.iter().map(|row| row.get(0)).collect();
            assert_eq!(labels, expected.into_iter().collect::<Vec<_>>());
            let read_count = |metrics: &serde_json::Value, metric: &str| {
                metrics["read_paths"][metric]
                    .as_u64()
                    .expect("read counter")
            };
            assert_eq!(
                read_count(&after, "index_seek_scans") - read_count(&before, "index_seek_scans"),
                1
            );
            assert_eq!(
                read_count(&after, "collection_scan_rows"),
                read_count(&before, "collection_scan_rows")
            );
        }

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_keep_materialized_view_reads_on_the_repeatable_read_generation() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("retain_materialized_view").await;
        let (writer, connection) = server.connect().await;
        let (reader, reader_connection) = server.connect().await;
        execute_statements(
            &writer,
            &[
                "CREATE TABLE mv_rr_source (k INT, label TEXT)",
                "INSERT INTO mv_rr_source VALUES (1, 'old')",
                "CREATE MATERIALIZED VIEW mv_rr AS SELECT k, label FROM mv_rr_source",
                "CREATE UNIQUE INDEX mv_rr_key ON mv_rr (k)",
            ],
        )
        .await
        .expect("create initial output");
        reader
            .batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ")
            .await
            .unwrap();
        let initial = reader
            .query_one("SELECT label FROM mv_rr WHERE k = 1", &[])
            .await
            .unwrap();

        // Act
        execute_statements(
            &writer,
            &[
                "UPDATE mv_rr_source SET label = 'new' WHERE k = 1",
                "REFRESH MATERIALIZED VIEW CONCURRENTLY mv_rr",
            ],
        )
        .await
        .expect("publish replacement while the reader is active");
        let point = reader
            .query("SELECT label FROM mv_rr WHERE k = $1", &[&1_i32])
            .await
            .unwrap();
        let scan = reader
            .query("SELECT label FROM mv_rr ORDER BY k", &[])
            .await
            .unwrap();
        reader.batch_execute("COMMIT").await.unwrap();
        let fresh = reader
            .query_one("SELECT label FROM mv_rr WHERE k = $1", &[&1_i32])
            .await
            .unwrap();

        // Assert
        assert_eq!(initial.get::<_, String>(0), "old");
        for rows in [point, scan] {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].get::<_, String>(0), "old");
        }
        assert_eq!(fresh.get::<_, String>(0), "new");

        drop(reader);
        reader_connection.await.unwrap();
        drop(writer);
        server.shutdown(connection).await;
    });
}
