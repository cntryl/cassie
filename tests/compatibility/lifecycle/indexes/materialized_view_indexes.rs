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
        assert!(!has_indexes);
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
